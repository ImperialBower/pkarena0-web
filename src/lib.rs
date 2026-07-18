//! WASM bindings for pkarena0 — single human player vs 8 bots in NLHE.
//!
//! Game state is held in three `thread_local!` singletons so the JS side can
//! call simple functions without passing state back and forth.

use std::cell::RefCell;

use pkcore::analysis::eval::Eval;
use pkcore::analysis::name::HandRankName;
use pkcore::analysis::player_stats::{Confidence, PlayerStats, StatsRegistry};
use pkcore::arrays::seven::Seven;
use pkcore::bot::decider::{BotDecider, JokerDecider, RuleBasedDecider};
use pkcore::bot::decision_config::{DecisionConfig, EquityMode, RangeMode};
use pkcore::bot::exploit::ExploitConfig;
use pkcore::bot::exploitative_decider::ExploitativeDecider;
use pkcore::bot::profile::BotProfile;
use pkcore::bot::table_snapshot::TableSnapshot;
use pkcore::card::Card;
use pkcore::cards::Cards;
use pkcore::casino::action::PlayerAction;
use pkcore::casino::action::TableAction;
use pkcore::casino::game::ForcedBets;
use pkcore::casino::session::PokerSession;
use pkcore::casino::state::PlayerState;
use pkcore::casino::table::{Player, Seat, Seats, Table};
use pkcore::casino::winnings::Winnings;
use pkcore::games::GamePhase;
use pkcore::hand_history::{
    Action as HhAction, ActionType, HandCollection, HandHistory, Outcome, PlayerSnapshot,
};
use pkcore::suit::Suit;
use rand::SeedableRng;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use wasm_bindgen::prelude::*;

// ── Thread-local game state ───────────────────────────────────────────────────

#[derive(Default, PartialEq, Clone, Copy)]
enum SessionPhase {
    #[default]
    Uninitialized,
    /// Bots have pending actions; JS is stepping through them one at a time.
    BotsActing,
    WaitingForHuman,
    /// Hand ended; cards still intact — JS shows results before next hand.
    HandComplete,
    SessionOver,
}

struct BotSeat {
    profile: BotProfile,
    decider: Box<dyn BotDecider>,
}

/// A named bot-lineup bundle (EPIC-49 Phase 1). Loaded from embedded YAML at
/// session start; the profile pool is shuffled from `profiles`. A wrapper
/// (rather than a bare `Vec`) leaves room for tier metadata (weak/standard/
/// strong) and forward-compatible fields.
#[derive(serde::Serialize, serde::Deserialize)]
struct BotBundle {
    #[serde(default)]
    name: String,
    profiles: Vec<BotProfile>,
}

thread_local! {
    static SESSION: RefCell<Option<PokerSession>> = const { RefCell::new(None) };
    static BOTS: RefCell<Vec<BotSeat>> = RefCell::new(Vec::new());
    static RNG: RefCell<SmallRng> = RefCell::new(SmallRng::seed_from_u64(0));
    static PHASE: RefCell<SessionPhase> = const { RefCell::new(SessionPhase::Uninitialized) };
    /// Chip counts at the start of the current hand (before blinds), indexed by seat.
    static HAND_START_CHIPS: RefCell<Vec<(u8, usize)>> = const { RefCell::new(Vec::new()) };
    /// Accumulated hand histories for the session; exported via get_session_yaml().
    static COLLECTION: RefCell<HandCollection> = RefCell::new(HandCollection::new());
    /// Session-scoped opponent stats (EPIC-47 Phase 1). Fed one `HandHistory`
    /// per completed hand, keyed by `Player` `Uuid`. Reset per session alongside
    /// `COLLECTION`. Populated but not yet read — later phases
    /// (injection/adaptation/HUD) consume it. `StatsRegistry::new()` is not
    /// `const`, so this mirrors `COLLECTION`'s non-const initializer.
    static REGISTRY: RefCell<StatsRegistry> = RefCell::new(StatsRegistry::new());
    /// One-shot error message surfaced to the UI without locking the game.
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    /// One-shot hand result populated by next_hand(), consumed by build_game_state().
    static LAST_HAND_RESULT: RefCell<Option<Vec<PotResult>>> = const { RefCell::new(None) };
    /// One-shot showdown reveal populated by next_hand() (only when 2+ players
    /// reached showdown), consumed by build_game_state().
    static LAST_SHOWDOWN: RefCell<Option<Vec<ShowdownPlayer>>> = const { RefCell::new(None) };
    /// When true, seat 0 is a bot (Arena mode); step_bot() never sets WaitingForHuman.
    static IS_ALL_BOT: RefCell<bool> = const { RefCell::new(false) };
    /// EPIC-47 Phase 3: when true, bot deciders are wrapped in
    /// `ExploitativeDecider` so they adapt to observed opponent stats. Read at
    /// bot-construction time (`init_game` / `init_bot_game`); changing it via
    /// `set_adaptive()` takes effect on the next New Game / Start Arena. Default
    /// on for both modes; the Settings toggle flips it.
    static ADAPTIVE: RefCell<bool> = const { RefCell::new(true) };
    /// Count of bot actions rejected by the engine and force-converted to Fold.
    /// This is EXPECTED to be nonzero, not just a bug tripwire: `RuleBasedDecider`
    /// routinely sizes a raise below the NLHE minimum increment, which the engine
    /// rejects with `InsufficientIncrement`. Baseline is ~2 per 20-hand arena run
    /// (range 0–6). See `docs/known-issues.md`; do not assert this equals 0.
    static FORCED_FOLD_COUNT: RefCell<u32> = const { RefCell::new(0) };
    /// EPIC-49 Phase 3: difficulty tier for the NEXT lineup build. Selects the
    /// profile bundle and gates adaptivity (weak: off, standard: the
    /// `ADAPTIVE` toggle, strong: forced on). Like `ADAPTIVE`, read at
    /// `init_game` / `init_bot_game`, so a change applies on the next New
    /// Game / Start Arena.
    static DIFFICULTY: RefCell<Difficulty> = const { RefCell::new(Difficulty::Standard) };
    /// Chip counts at session start, one entry per originally seated player
    /// (seat, name, chips). Unlike `HAND_START_CHIPS` this never advances, so
    /// the session chips/100 report can net busted seats (EPIC-49 Phase 3).
    static SESSION_START_CHIPS: RefCell<Vec<(u8, String, usize)>> = const { RefCell::new(Vec::new()) };
    /// Within-hand undo stack: one entry pushed before each human action.
    /// Cleared at every `next_hand()` boundary (no rewinding across a completed
    /// hand once chips have been distributed). See `Snapshot` / `undo_action`.
    static HISTORY: RefCell<Vec<Snapshot>> = const { RefCell::new(Vec::new()) };
}

// ── Within-hand undo snapshot ─────────────────────────────────────────────────
//
// Snapshot the mutable per-hand state before each human action; restore it in
// `undo_action()`. Ported from the April `rewind` branch (pkcore 0.0.48). Two
// things changed under it since then, both handled here:
//   • `Snapshot.table` is now `Table` (was `TableNoCell`).
//   • EPIC-49 added `FORCED_FOLD_COUNT`, which ticks mid-hand as bots act — it
//     is snapshotted so an undo also un-counts any bot forced-folds that
//     happened after the action being undone.
// Deliberately NOT snapshotted: `BOTS` and `REGISTRY`. `BotDecider::decide_seeded`
// takes `&self` (verified stateless in pkcore 0.3.0), so bots replay
// deterministically from the restored `RNG`; `REGISTRY` only ingests a hand at
// completion, never inside the snapshotted window (history clears at next_hand).
struct Snapshot {
    table: Table,
    hand_number: u32,
    shuffled_deck_str: Option<String>,
    rng: SmallRng,
    phase: SessionPhase,
    hand_start_chips: Vec<(u8, usize)>,
    forced_fold_count: u32,
}

fn push_snapshot() {
    let snap = SESSION.with(|s| {
        s.borrow().as_ref().map(|sess| {
            (
                sess.table.clone(),
                sess.hand_number,
                sess.shuffled_deck_str.clone(),
            )
        })
    });
    if let Some((table, hand_number, shuffled_deck_str)) = snap {
        let rng = RNG.with(|r| r.borrow().clone());
        let phase = PHASE.with(|p| *p.borrow());
        let hand_start_chips = HAND_START_CHIPS.with(|h| h.borrow().clone());
        let forced_fold_count = FORCED_FOLD_COUNT.with(|c| *c.borrow());
        HISTORY.with(|h| {
            h.borrow_mut().push(Snapshot {
                table,
                hand_number,
                shuffled_deck_str,
                rng,
                phase,
                hand_start_chips,
                forced_fold_count,
            })
        });
    }
}

/// EPIC-49 Phase 3 difficulty tiers. `Weak` plays the dampened bundle with
/// adaptation off; `Standard` plays the standard bundle; `Strong` plays the
/// sharpened bundle (`strengthen`) — the interim "strong" lever until
/// upstream pkcore EPIC-36 ships real capability knobs. Standard and strong
/// both honor the EPIC-47 adaptive toggle (see `effective_adaptive`).
#[derive(Clone, Copy, Default, PartialEq)]
enum Difficulty {
    Weak,
    #[default]
    Standard,
    Strong,
}

impl Difficulty {
    fn parse(level: &str) -> Option<Self> {
        match level {
            "weak" => Some(Self::Weak),
            "standard" => Some(Self::Standard),
            "strong" => Some(Self::Strong),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Weak => "weak",
            Self::Standard => "standard",
            Self::Strong => "strong",
        }
    }
}

// ── WASM entry point ──────────────────────────────────────────────────────────

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
}

// ── Public WASM exports ───────────────────────────────────────────────────────

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// EPIC-47 Phase 3: enable/disable adaptive (exploitative) bots.
///
/// When enabled, each bot's decider is wrapped in `ExploitativeDecider` at the
/// start of the next hand-lineup build, so it deviates from its baseline
/// profile based on the largest opponent's observed stats. The change is read
/// when the lineup is constructed, so it takes effect on the next `init_game`
/// / `init_bot_game` (New Game / Start Arena), not mid-session.
#[wasm_bindgen]
pub fn set_adaptive(enabled: bool) {
    ADAPTIVE.with(|a| *a.borrow_mut() = enabled);
}

/// Current adaptive-bots preference (see `set_adaptive`). Lets the UI restore
/// the Settings toggle to its true state on load.
#[wasm_bindgen]
pub fn adaptive_enabled() -> bool {
    ADAPTIVE.with(|a| *a.borrow())
}

/// EPIC-49 Phase 3: select the difficulty tier (`"weak"` / `"standard"` /
/// `"strong"`) for the next lineup build. Like `set_adaptive`, the value is
/// read when a lineup is constructed, so it applies on the next New Game /
/// Start Arena. Unknown levels are rejected with a console warning and the
/// current tier is kept. Returns `true` when the level was accepted.
#[wasm_bindgen]
pub fn set_difficulty(level: &str) -> bool {
    match Difficulty::parse(level) {
        Some(d) => {
            DIFFICULTY.with(|c| *c.borrow_mut() = d);
            true
        }
        None => {
            console_warn(&format!(
                "unknown difficulty {level:?}; keeping current tier"
            ));
            false
        }
    }
}

/// Current difficulty tier (see `set_difficulty`). Lets the UI restore the
/// Settings selector to its true state on load.
#[wasm_bindgen]
pub fn difficulty_level() -> String {
    DIFFICULTY.with(|d| d.borrow().as_str().to_string())
}

// EPIC-48 note: the Phase 0 `equity_probe` export (in-browser latency spike
// for pkcore's equity engine) was removed when the EPIC closed — its numbers
// are recorded in docs/EPIC-48_Real_Equity_WASM.md and the probe lives in git
// history (`git log -S equity_probe`). The `equity` cargo feature stays on so
// the wasm build keeps pre-verifying the engine compiles for the day upstream
// pkcore EPIC-36 wires it into the decider.

/// Initialise a new session with 9 players (seat 0 = human, seats 1-8 = bots).
///
/// Seeds the RNG from `rand_seed`, deals the first hand, and advances bots
/// until it is the human's turn. Returns a `GameState` JSON string.
#[wasm_bindgen]
pub fn init_game(rand_seed: f64) -> String {
    IS_ALL_BOT.with(|f| *f.borrow_mut() = false);
    // Seed RNG.
    RNG.with(|r| *r.borrow_mut() = SmallRng::seed_from_u64(rand_seed.to_bits()));

    // Build the table: hero at seat 0, bots after. The difficulty tier picks
    // the embedded lineup bundle (EPIC-49 Phase 3); shuffled so it varies each
    // game. The weak pool has 8 profiles (no joker), others 9 — take(8) seats
    // whatever is available.
    let difficulty = DIFFICULTY.with(|d| *d.borrow());
    let mut profile_pool = profiles_for(difficulty);
    RNG.with(|r| profile_pool.shuffle(&mut *r.borrow_mut()));
    let adaptive = effective_adaptive(difficulty);
    let bots: Vec<BotSeat> = profile_pool
        .into_iter()
        .take(8)
        .map(|p| make_bot_seat(p, adaptive))
        .collect();
    let bot_names: Vec<String> = bots.iter().map(|b| b.profile.name.clone()).collect();

    let mut seats_vec = vec![Seat::new(Player::new_with_chips("You".to_string(), 10_000))];
    for name in &bot_names {
        seats_vec.push(Seat::new(Player::new_with_chips(name.clone(), 10_000)));
    }

    // Capture chip counts BEFORE start_hand() posts blinds.
    let start_chips: Vec<(u8, usize)> = seats_vec
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u8, s.player.chips))
        .collect();
    HAND_START_CHIPS.with(|h| *h.borrow_mut() = start_chips);
    record_session_start(&seats_vec);
    COLLECTION.with(|c| *c.borrow_mut() = HandCollection::new());
    REGISTRY.with(|r| *r.borrow_mut() = StatsRegistry::new());
    FORCED_FOLD_COUNT.with(|c| *c.borrow_mut() = 0);
    // A new game starts with no undo history; stale snapshots would point at
    // the previous session's table.
    HISTORY.with(|h| h.borrow_mut().clear());

    let table = Table::nlh_from_seats(Seats::new(seats_vec), ForcedBets::new(50, 100));

    let mut session = PokerSession::new(table);
    if session.start_hand().is_err() {
        return error_state("Failed to deal first hand");
    }

    BOTS.with(|b| *b.borrow_mut() = bots);
    notify_bots_new_hand();
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    PHASE.with(|p| *p.borrow_mut() = SessionPhase::BotsActing);

    build_game_state()
}

/// Initialise an all-bot Arena session with 9 bots (no human player).
///
/// All seats are filled by bots; `step_bot()` will never pause for human input.
/// Returns a `GameState` JSON string.
#[wasm_bindgen]
pub fn init_bot_game(rand_seed: f64) -> String {
    IS_ALL_BOT.with(|f| *f.borrow_mut() = true);
    RNG.with(|r| *r.borrow_mut() = SmallRng::seed_from_u64(rand_seed.to_bits()));

    // Fill every seat with a bot (seat 0 included). The difficulty tier picks
    // the bundle (EPIC-49 Phase 3): 9 seats standard/strong, 8 weak (no joker).
    let difficulty = DIFFICULTY.with(|d| *d.borrow());
    let mut profile_pool = profiles_for(difficulty);
    RNG.with(|r| profile_pool.shuffle(&mut *r.borrow_mut()));
    let adaptive = effective_adaptive(difficulty);
    let bots: Vec<BotSeat> = profile_pool
        .into_iter()
        .take(9)
        .map(|p| make_bot_seat(p, adaptive))
        .collect();
    let bot_names: Vec<String> = bots.iter().map(|b| b.profile.name.clone()).collect();

    let seats_vec: Vec<Seat> = bot_names
        .iter()
        .map(|name| Seat::new(Player::new_with_chips(name.clone(), 10_000)))
        .collect();

    let start_chips: Vec<(u8, usize)> = seats_vec
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u8, s.player.chips))
        .collect();
    HAND_START_CHIPS.with(|h| *h.borrow_mut() = start_chips);
    record_session_start(&seats_vec);
    COLLECTION.with(|c| *c.borrow_mut() = HandCollection::new());
    REGISTRY.with(|r| *r.borrow_mut() = StatsRegistry::new());
    FORCED_FOLD_COUNT.with(|c| *c.borrow_mut() = 0);
    // A new game starts with no undo history; stale snapshots would point at
    // the previous session's table.
    HISTORY.with(|h| h.borrow_mut().clear());

    let table = Table::nlh_from_seats(Seats::new(seats_vec), ForcedBets::new(50, 100));

    let mut session = PokerSession::new(table);
    if session.start_hand().is_err() {
        return error_state("Failed to deal first hand");
    }

    BOTS.with(|b| *b.borrow_mut() = bots);
    notify_bots_new_hand();
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    PHASE.with(|p| *p.borrow_mut() = SessionPhase::BotsActing);

    build_game_state()
}

/// Update forced bets. If a hand is in progress the change is deferred until
/// the hand ends — this keeps mid-hand `min_raise()` validation stable and
/// guarantees the recorded `stakes` match the actual posts in hand history.
/// Returns updated GameState JSON.
#[wasm_bindgen]
pub fn set_blinds(small_blind: f64, big_blind: f64) -> String {
    SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.set_blinds(ForcedBets::new(small_blind as usize, big_blind as usize));
        }
    });
    build_game_state()
}

/// Apply a human action and advance bots until the human's next turn.
///
/// Input is a JSON string: `{ "action": "Bet", "amount": 300 }`.
/// While the current phase is `HandComplete`, any call to this function
/// advances to the next hand instead of applying an action.
#[wasm_bindgen]
pub fn human_action(action_json: &str) -> String {
    let current_phase = PHASE.with(|p| *p.borrow());
    match current_phase {
        SessionPhase::Uninitialized => return error_state("Call init_game first"),
        SessionPhase::SessionOver => return build_game_state(),
        SessionPhase::BotsActing => return build_game_state(),
        SessionPhase::HandComplete => {
            // Treat any action while the hand is complete as "advance to next hand".
            return next_hand();
        }
        SessionPhase::WaitingForHuman => {}
    }

    let req: ActionRequest = match serde_json::from_str(action_json) {
        Ok(r) => r,
        Err(e) => return error_state(&format!("Bad action JSON: {e}")),
    };

    let action = match req.action.as_str() {
        "Fold" => PlayerAction::Fold,
        "Check" => PlayerAction::Check,
        "Call" => PlayerAction::Call,
        "Bet" => PlayerAction::Bet(req.amount),
        "Raise" => PlayerAction::Raise(req.amount),
        "AllIn" => PlayerAction::AllIn,
        other => return error_state(&format!("Unknown action: {other}")),
    };

    // Snapshot before mutating so `undo_action()` can step back to this exact
    // decision point. Popped again below if the action turns out illegal.
    push_snapshot();
    let apply_result = SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.apply_action(0, action).err().map(|e| e.to_string())
        } else {
            Some("No active session".to_string())
        }
    });

    if let Some(err) = apply_result {
        // Action was rejected — discard the snapshot we just pushed so undo
        // doesn't step back to an identical (unchanged) state.
        HISTORY.with(|h| {
            h.borrow_mut().pop();
        });
        // Store the error so build_game_state() can surface it, but keep the
        // phase as WaitingForHuman so the action buttons remain usable.
        LAST_ERROR.with(|e| *e.borrow_mut() = Some(err));
        return build_game_state();
    }

    PHASE.with(|p| *p.borrow_mut() = SessionPhase::BotsActing);
    build_game_state()
}

/// Undo the last applied action within the current hand.
///
/// Restores the game to the state it was in immediately before the most recent
/// `human_action` call that mutated session state. Returns the restored
/// `GameState` JSON. If there is nothing to undo, returns the current state
/// unchanged (no error). History is cleared at each `next_hand()` boundary, so
/// rewinding across a completed hand is not supported here.
///
/// Bots that acted after the undone action are replayed deterministically from
/// the restored `RNG` on the next `step_bot` loop, because `BotDecider` is
/// stateless (`decide_seeded(&self, …)`).
#[wasm_bindgen]
pub fn undo_action() -> String {
    let snap = HISTORY.with(|h| h.borrow_mut().pop());
    let Some(snap) = snap else {
        return build_game_state();
    };
    SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.table = snap.table;
            session.hand_number = snap.hand_number;
            session.shuffled_deck_str = snap.shuffled_deck_str;
        }
    });
    RNG.with(|r| *r.borrow_mut() = snap.rng);
    PHASE.with(|p| *p.borrow_mut() = snap.phase);
    HAND_START_CHIPS.with(|h| *h.borrow_mut() = snap.hand_start_chips);
    FORCED_FOLD_COUNT.with(|c| *c.borrow_mut() = snap.forced_fold_count);
    LAST_ERROR.with(|e| *e.borrow_mut() = None);
    build_game_state()
}

/// Advance from a completed hand to the next one.
///
/// Calls `end_hand`, removes busted players, moves the button, and deals a
/// fresh hand. Returns `GameState` JSON.
#[wasm_bindgen]
pub fn next_hand() -> String {
    let current_phase = PHASE.with(|p| *p.borrow());
    if current_phase != SessionPhase::HandComplete {
        return build_game_state();
    }
    // Undo does not cross hand boundaries: chips are about to be distributed.
    HISTORY.with(|h| h.borrow_mut().clear());

    // ── Snapshot everything we need BEFORE end_hand() mucks cards ────────────
    struct PreEnd {
        hand_num: usize,
        button: u8,
        forced: ForcedBets,
        board_str: String,
        event_log: Vec<TableAction>,
        player_snapshot: Vec<PlayerSnapshot>,
        shuffled_deck_str: Option<String>,
        showdown: Option<Vec<ShowdownPlayer>>,
    }

    let snap: Option<PreEnd> = SESSION.with(|s| {
        s.borrow().as_ref().map(|session| {
            let table = &session.table;
            let start_chips = HAND_START_CHIPS.with(|h| h.borrow().clone());

            let player_snapshot = table
                .seats
                .0
                .iter()
                .enumerate()
                .filter_map(|(i, seat)| {
                    if seat.is_empty() {
                        return None;
                    }
                    let seat_num = i as u8;
                    let starting = start_chips
                        .iter()
                        .find(|(s, _)| *s == seat_num)
                        .map_or(0, |(_, c)| *c);
                    // Use dealt_hole_cards (survives folds) so folders' cards
                    // appear in the hand history, not just the winner's.
                    let hole_str = table.dealt_hole_cards.get(&seat_num).and_then(|bc| {
                        let s: String = sorted_hand(bc.as_slice())
                            .iter()
                            .map(|c| c.to_string())
                            .collect::<Vec<_>>()
                            .join(" ");
                        if s.is_empty() { None } else { Some(s) }
                    });
                    Some((seat_num, seat.player.handle.clone(), starting, hole_str, Some(seat.player.id)))
                })
                .collect();

            // Reveal every seat still in the hand (2+ = genuine showdown).
            // All-in players are included. Evaluate each seat's 7 cards for its
            // hand category. Skipped (None) when it was a fold-out.
            let active = table.seats.active_in_hand();
            let showdown: Option<Vec<ShowdownPlayer>> = if active.len() >= 2 {
                let players = active
                    .iter()
                    .filter_map(|&seat_num| {
                        let seat = table.seats.get_seat(seat_num)?;
                        let cards: Vec<String> = table
                            .dealt_hole_cards
                            .get(&seat_num)
                            .map(|bc| sorted_hand(bc.as_slice()).iter().map(card_to_str).collect())
                            .unwrap_or_default();
                        let hand = table
                            .effective_player_cards(seat_num)
                            .and_then(|c| Seven::try_from(c).ok())
                            .and_then(|seven| {
                                hand_rank_name_to_str(Eval::from(seven).hand_rank.name)
                            })
                            .unwrap_or_default();
                        Some(ShowdownPlayer {
                            seat: seat_num,
                            name: seat.player.handle.clone(),
                            cards,
                            hand,
                        })
                    })
                    .collect();
                Some(players)
            } else {
                None
            };

            PreEnd {
                hand_num: session.hand_number as usize,
                button: table.button,
                forced: session.forced_at_hand_start(),
                board_str: table
                    .board
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
                event_log: table.event_log.clone(),
                player_snapshot,
                shuffled_deck_str: session.shuffled_deck_str.clone(),
                showdown,
            }
        })
    });

    // ── end_hand: distributes winnings, then resets board/cards ──────────────
    let winnings_result: Result<Winnings, String> = SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.end_hand().map_err(|e| e.to_string())
        } else {
            Err("No active session".to_string())
        }
    });

    // pkcore bug: in some multiway showdown scenarios orphaned NONE equity
    // entries (folded players' chips above an all-in winner's level) are not
    // distributed, causing the chip audit to fail.  Table::reset() already ran
    // before the audit, so the table is in a clean state.  Surface the error as
    // a warning and continue the session rather than freezing the UI.
    let had_audit_failure;
    let winnings = match winnings_result {
        Ok(w) => {
            had_audit_failure = false;
            w
        }
        Err(err) if err.contains("Chip audit failed") => {
            had_audit_failure = true;
            LAST_ERROR.with(|e| *e.borrow_mut() = Some(format!("Engine error: {err}")));
            Winnings::default()
        }
        Err(err) => return error_state(&err),
    };

    // ── Read ending stacks, record history, prime next hand's starting chips ──
    if let Some(s) = snap {
        SESSION.with(|sess| {
            if let Some(session) = sess.borrow().as_ref() {
                let ending_stacks: Vec<(u8, usize)> = session
                    .table
                    .seats
                    .0
                    .iter()
                    .enumerate()
                    .filter_map(|(i, seat)| {
                        if seat.is_empty() {
                            return None;
                        }
                        Some((i as u8, seat.player.chips))
                    })
                    .collect();

                // Store as starting chips for the next hand.
                HAND_START_CHIPS.with(|h| *h.borrow_mut() = ending_stacks.clone());

                // Skip hand history and winner display when the chip audit failed;
                // the winnings are either absent or unreliable.
                if !had_audit_failure {
                    let hh = HandHistory::from_table_state_with_ids(
                        s.hand_num,
                        0, // ts_secs — no wall clock in WASM
                        s.button,
                        &s.forced,
                        &s.player_snapshot,
                        &s.board_str,
                        &winnings,
                        &s.event_log,
                        &ending_stacks,
                        "pkarena0",
                        s.shuffled_deck_str,
                    );
                    // Ingest BEFORE push: HandCollection::push takes `hh` by
                    // value (moves it), so the stats registry must borrow it
                    // first. Registry is inert until a later phase reads it.
                    REGISTRY.with(|r| r.borrow_mut().ingest_hand(&hh));
                    COLLECTION.with(|c| c.borrow_mut().push(hh));

                    // Build per-pot winner summary for the UI.
                    let pot_results: Vec<PotResult> = winnings
                        .vec()
                        .iter()
                        .map(|pot_win| {
                            let seats: Vec<u8> = (0u8..9)
                                .filter(|&i| pot_win.equity.seats.contains(i))
                                .collect();
                            let names: Vec<String> = seats
                                .iter()
                                .map(|&seat| {
                                    s.player_snapshot
                                        .iter()
                                        .find(|(sn, _, _, _, _)| *sn == seat)
                                        .map(|(_, name, _, _, _)| name.clone())
                                        .unwrap_or_default()
                                })
                                .collect();
                            PotResult {
                                seats,
                                names,
                                amount: pot_win.equity.chips,
                                hand: hand_rank_name_to_str(pot_win.eval.hand_rank.name),
                            }
                        })
                        .collect();
                    LAST_HAND_RESULT.with(|r| *r.borrow_mut() = Some(pot_results));
                    LAST_SHOWDOWN.with(|r| *r.borrow_mut() = s.showdown.clone());
                }
            }
        });
    }

    // pkcore's Table::reset() does not clear event_log, so it accumulates
    // across every hand.  Clear it here, after the hand history snapshot has
    // been recorded, so each new hand starts with a clean log.
    SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.table.event_log.clear();
        }
    });

    SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.eliminate_busted();
            session.table.button_up();
            // pkcore's Table::button_up() increments by 1 mod the full
            // seat array (9), not the next occupied seat.  After busts leave
            // gaps, determine_small_blind() resolves the button to the first
            // occupied seat at-or-after that index, so a head-up pair on
            // seats 0 and 7 ends up with seat 7 paying SB on 7 of every 9
            // button positions.  Walk the button forward until it lands on
            // an occupied seat to restore fair blind alternation.
            let total = session.table.seats.0.len();
            for _ in 0..total {
                let idx = session.table.button;
                let occupied = session
                    .table
                    .seats
                    .get_seat(idx)
                    .is_some_and(|seat| !seat.is_empty());
                if occupied {
                    break;
                }
                session.table.button_up();
            }
        }
    });

    let funded = SESSION.with(|s| s.borrow().as_ref().map_or(0, |sess| sess.count_funded()));

    if funded < 2 {
        PHASE.with(|p| *p.borrow_mut() = SessionPhase::SessionOver);
        return build_game_state();
    }

    let start_result: Option<String> = SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.start_hand().err().map(|e| e.to_string())
        } else {
            Some("No active session".to_string())
        }
    });

    if let Some(err) = start_result {
        return error_state(&err);
    }

    notify_bots_new_hand();

    PHASE.with(|p| *p.borrow_mut() = SessionPhase::BotsActing);
    build_game_state()
}

/// Return the current game state as JSON without advancing anything.
#[wasm_bindgen]
pub fn get_state() -> String {
    build_game_state()
}

/// Return all completed hand histories for this session as a YAML string.
///
/// Returns an empty collection YAML if no hands have completed yet.
#[wasm_bindgen]
pub fn get_session_yaml() -> String {
    COLLECTION.with(|c| {
        c.borrow()
            .to_yaml()
            .unwrap_or_else(|_| "error: yaml serialization failed\n".to_string())
    })
}

/// Parse a YAML string (HandCollection or single HandHistory) and return a JSON
/// summary of each hand suitable for populating the replay viewer's hand picker.
///
/// On parse error, returns an `error_state` JSON with the error message.
#[wasm_bindgen]
pub fn parse_hand_collection(yaml: &str) -> String {
    let coll = match parse_collection_or_single(yaml) {
        Ok(c) => c,
        Err(e) => return error_state(&format!("YAML parse error: {e}")),
    };
    let hands: Vec<HandSummary> = coll
        .hands
        .iter()
        .enumerate()
        .map(|(idx, h)| {
            let total_steps = compute_total_steps(h);
            let button = h.table.button.unwrap_or(0);
            let hand_id = h.hand.id.clone();
            let description = format_hand_summary(h);
            HandSummary {
                index: idx,
                hand_id,
                total_steps,
                button_seat: button,
                description,
            }
        })
        .collect();
    serde_json::to_string(&CollectionSummary { hands })
        .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string())
}

/// Compute a snapshot of the given hand at the given step, returned as JSON.
///
/// `step` is clamped into `[0, total_steps - 1]`.  Step 0 is the state right
/// after blinds are posted; subsequent steps apply each voluntary action and
/// each street deal in sequence.
#[wasm_bindgen]
pub fn replay_snapshot(yaml: &str, hand_index: usize, step: usize) -> String {
    let coll = match parse_collection_or_single(yaml) {
        Ok(c) => c,
        Err(e) => return error_state(&format!("YAML parse error: {e}")),
    };
    let Some(hh) = coll.hands.get(hand_index) else {
        return error_state(&format!("hand index {hand_index} out of range"));
    };
    match build_replay_snapshot(hh, step) {
        Ok(snap) => serde_json::to_string(&snap)
            .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string()),
        Err(e) => error_state(&format!("Replay error: {e}")),
    }
}

fn parse_collection_or_single(yaml: &str) -> Result<HandCollection, String> {
    if let Ok(c) = HandCollection::from_yaml(yaml) {
        return Ok(c);
    }
    match HandHistory::from_yaml(yaml) {
        Ok(h) => {
            let mut c = HandCollection::new();
            c.hands.push(h);
            Ok(c)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn format_hand_summary(hh: &HandHistory) -> String {
    let player_count = hh.players.len();
    let button = hh.table.button.unwrap_or(0);
    let mut desc = format!("BTN Seat {button}, {player_count} handed");

    let Some(results) = hh.results.as_deref() else {
        return desc;
    };
    let winner = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Win | Outcome::Tie))
        .max_by(|a, b| {
            a.pot_won
                .unwrap_or(0.0)
                .partial_cmp(&b.pot_won.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    let Some(w) = winner else {
        return desc;
    };

    let winner_name = hh
        .players
        .iter()
        .find(|p| p.seat == w.seat)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| format!("Seat {}", w.seat));
    let verb = if winner_name == "You" { "win" } else { "wins" };
    let cards = winner_cards_pretty(hh, w.seat);
    if cards.is_empty() {
        desc.push_str(&format!(", {winner_name} {verb}"));
    } else {
        desc.push_str(&format!(", {winner_name} {verb} with {cards}"));
    }
    desc
}

fn winner_cards_pretty(hh: &HandHistory, seat: u8) -> String {
    let hole = hh
        .players
        .iter()
        .find(|p| p.seat == seat)
        .and_then(|p| p.hole_cards.as_deref())
        .unwrap_or("");
    let board = hh.board.as_deref().unwrap_or("");

    let combined = match (hole.is_empty(), board.is_empty()) {
        (true, true) => return String::new(),
        (false, true) => hole.to_string(),
        (true, false) => board.to_string(),
        (false, false) => format!("{hole} {board}"),
    };

    combined
        .split_whitespace()
        .map(card_token_to_unicode)
        .collect::<Vec<_>>()
        .join(" ")
}

fn card_token_to_unicode(token: &str) -> String {
    let mut chars = token.chars();
    let Some(rank) = chars.next() else {
        return String::new();
    };
    let suit_char = chars.next().unwrap_or(' ');
    let rank_str = if rank == 'T' {
        "10".to_string()
    } else {
        rank.to_string()
    };
    let suit = match suit_char {
        's' | 'S' | '\u{2660}' => "\u{2660}",
        'h' | 'H' | '\u{2665}' => "\u{2665}",
        'd' | 'D' | '\u{2666}' => "\u{2666}",
        'c' | 'C' | '\u{2663}' => "\u{2663}",
        _ => "",
    };
    format!("{rank_str}{suit}")
}

fn compute_total_steps(hh: &HandHistory) -> usize {
    let mut steps = 1; // step 0 = initial state after blinds posted
    let Some(streets) = &hh.streets else {
        return steps;
    };
    if let Some(pre) = &streets.preflop {
        steps += pre
            .actions
            .iter()
            .filter(|a| !matches!(a.action, ActionType::Post))
            .count();
    }
    if let Some(flop) = &streets.flop {
        steps += 1 + flop.actions.len();
    }
    if let Some(turn) = &streets.turn {
        steps += 1 + turn.actions.len();
    }
    if let Some(river) = &streets.river {
        steps += 1 + river.actions.len();
    }
    steps
}

enum ReplayEvent {
    Action {
        seat: u8,
        action: PlayerAction,
        label: String,
    },
    DealFlop(String),
    DealTurn(String),
    DealRiver(String),
}

fn build_event_list(hh: &HandHistory) -> Vec<ReplayEvent> {
    let mut events = Vec::new();
    let Some(streets) = &hh.streets else {
        return events;
    };

    let push_actions = |events: &mut Vec<ReplayEvent>, actions: &[HhAction]| {
        for a in actions {
            if let Some(pa) = action_to_player_action(a) {
                events.push(ReplayEvent::Action {
                    seat: a.seat,
                    action: pa,
                    label: format_action_label(hh, a),
                });
            }
        }
    };

    if let Some(pre) = &streets.preflop {
        push_actions(&mut events, &pre.actions);
    }
    if let Some(flop) = &streets.flop {
        events.push(ReplayEvent::DealFlop(flop.cards.clone()));
        push_actions(&mut events, &flop.actions);
    }
    if let Some(turn) = &streets.turn {
        events.push(ReplayEvent::DealTurn(turn.card.clone()));
        push_actions(&mut events, &turn.actions);
    }
    if let Some(river) = &streets.river {
        events.push(ReplayEvent::DealRiver(river.card.clone()));
        push_actions(&mut events, &river.actions);
    }
    events
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn action_to_player_action(a: &HhAction) -> Option<PlayerAction> {
    match a.action {
        ActionType::Fold => Some(PlayerAction::Fold),
        ActionType::Check => Some(PlayerAction::Check),
        ActionType::Call => Some(PlayerAction::Call),
        ActionType::Bet => a.amount.map(|n| PlayerAction::Bet(n as usize)),
        ActionType::Raise => a.amount.map(|n| PlayerAction::Raise(n as usize)),
        ActionType::AllIn => Some(PlayerAction::AllIn),
        ActionType::Post => None,
        _ => None,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn format_action_label(hh: &HandHistory, a: &HhAction) -> String {
    let name = hh
        .players
        .iter()
        .find(|p| p.seat == a.seat)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| format!("Seat {}", a.seat));
    let amt = a.amount.unwrap_or(0.0) as usize;
    match a.action {
        ActionType::Fold => format!("{name} folds"),
        ActionType::Check => format!("{name} checks"),
        ActionType::Call => format!("{name} calls ${amt}"),
        ActionType::Bet => format!("{name} bets ${amt}"),
        ActionType::Raise => format!("{name} raises to ${amt}"),
        ActionType::AllIn => format!("{name} goes all-in"),
        ActionType::Post => format!("{name} posts ${amt}"),
        _ => format!("{name} acts"),
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
fn build_replay_snapshot(hh: &HandHistory, target_step: usize) -> Result<ReplaySnapshot, String> {
    let sb = hh.table.stakes.small_blind as usize;
    let bb = hh.table.stakes.big_blind as usize;
    let button = hh.table.button.unwrap_or(0);

    let max_seat = hh
        .players
        .iter()
        .map(|p| p.seat as usize)
        .max()
        .unwrap_or(0);
    let table_size = max_seat.max(button as usize) + 1;
    let mut seats_vec: Vec<Seat> = (0..table_size)
        .map(|_| Seat::new(Player::default()))
        .collect();
    for p in &hh.players {
        seats_vec[p.seat as usize] =
            Seat::new(Player::new_with_chips(p.name.clone(), p.stack as usize));
    }
    let seats = Seats::new(seats_vec);
    let mut table = Table::nlh_from_seats(seats, ForcedBets::new(sb, bb));
    table.button = button;

    table.act_forced_bets().map_err(|e| e.to_string())?;

    let hole_entries: Vec<(u8, String)> = hh
        .players
        .iter()
        .filter_map(|p| p.hole_cards.as_ref().map(|h| (p.seat, h.clone())))
        .collect();
    let hole_refs: Vec<(u8, &str)> = hole_entries.iter().map(|(s, h)| (*s, h.as_str())).collect();
    table
        .inject_hole_cards(&hole_refs)
        .map_err(|e| e.to_string())?;

    let events = build_event_list(hh);
    let total_steps = events.len() + 1;
    let target = target_step.min(events.len());

    let mut last_label = "Hand begins".to_string();
    let mut current_seat: Option<u8> = None;

    for event in events.iter().take(target) {
        match event {
            ReplayEvent::Action {
                seat,
                action,
                label,
            } => {
                table
                    .apply_action(*seat, action.clone())
                    .map_err(|e| e.to_string())?;
                last_label = label.clone();
                current_seat = Some(*seat);
            }
            ReplayEvent::DealFlop(cards) => {
                table.bring_it_in().map_err(|e| e.to_string())?;
                table.board = Cards::from_str(cards).map_err(|e| e.to_string())?;
                table.phase = GamePhase::DealFlop;
                last_label = format!("Flop dealt: {cards}");
                current_seat = None;
            }
            ReplayEvent::DealTurn(card) => {
                table.bring_it_in().map_err(|e| e.to_string())?;
                let c = Card::from_str(card).map_err(|e| e.to_string())?;
                table.board.insert(c);
                table.phase = GamePhase::DealTurn;
                last_label = format!("Turn dealt: {card}");
                current_seat = None;
            }
            ReplayEvent::DealRiver(card) => {
                table.bring_it_in().map_err(|e| e.to_string())?;
                let c = Card::from_str(card).map_err(|e| e.to_string())?;
                table.board.insert(c);
                table.phase = GamePhase::DealRiver;
                last_label = format!("River dealt: {card}");
                current_seat = None;
            }
        }
    }

    let dealer_seat = table.button;
    let sb_seat = table.determine_small_blind();
    let bb_seat = table.determine_big_blind();
    let board: Vec<String> = table.board.iter().map(card_to_str).collect();
    let pot_committed: usize =
        table.seats.0.iter().map(|s| s.player.bet).sum::<usize>() + table.pot;
    let street = street_from_board(table.board.len(), false);

    let replay_seats: Vec<ReplaySeat> = table
        .seats
        .0
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let seat = i as u8;
            if s.is_empty() {
                return ReplaySeat {
                    seat,
                    name: String::new(),
                    chips: 0,
                    bet: 0,
                    state: "Out".to_string(),
                    hole_cards: None,
                    is_dealer: false,
                    is_sb: false,
                    is_bb: false,
                };
            }
            let cards: Vec<String> = sorted_hand(s.cards.as_slice())
                .iter()
                .map(card_to_str)
                .collect();
            ReplaySeat {
                seat,
                name: s.player.handle.clone(),
                chips: s.player.chips,
                bet: s.player.bet,
                state: state_to_str(&s.player.state),
                hole_cards: if cards.is_empty() { None } else { Some(cards) },
                is_dealer: seat == dealer_seat,
                is_sb: seat == sb_seat,
                is_bb: seat == bb_seat,
            }
        })
        .collect();

    Ok(ReplaySnapshot {
        step: target,
        total_steps,
        action_label: last_label,
        current_seat,
        pot: pot_committed,
        board,
        dealer_seat,
        sb_seat,
        bb_seat,
        small_blind: sb,
        big_blind: bb,
        street,
        seats: replay_seats,
    })
}

#[derive(Serialize)]
struct HandSummary {
    index: usize,
    hand_id: String,
    total_steps: usize,
    button_seat: u8,
    description: String,
}

#[derive(Serialize)]
struct CollectionSummary {
    hands: Vec<HandSummary>,
}

#[derive(Serialize)]
struct ReplaySeat {
    seat: u8,
    name: String,
    chips: usize,
    bet: usize,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hole_cards: Option<Vec<String>>,
    is_dealer: bool,
    is_sb: bool,
    is_bb: bool,
}

#[derive(Serialize)]
struct ReplaySnapshot {
    step: usize,
    total_steps: usize,
    action_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_seat: Option<u8>,
    pot: usize,
    board: Vec<String>,
    dealer_seat: u8,
    sb_seat: u8,
    bb_seat: u8,
    small_blind: usize,
    big_blind: usize,
    street: String,
    seats: Vec<ReplaySeat>,
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ActionRequest {
    action: String,
    #[serde(default)]
    amount: usize,
}

/// Per-pot winner summary, included in `GameState.last_result` immediately after a hand ends.
#[derive(Serialize, Clone)]
struct PotResult {
    seats: Vec<u8>,
    names: Vec<String>,
    amount: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    hand: Option<String>,
}

/// One revealed hand at a genuine showdown (2+ players still in the hand).
/// Included in `GameState.showdown` immediately after such a hand ends.
#[derive(Serialize, Clone)]
struct ShowdownPlayer {
    seat: u8,
    name: String,
    cards: Vec<String>, // two-char codes, e.g. ["9s","As"]; JS renders [9♠ A♠]
    hand: String,       // evaluated category, e.g. "Two Pair" ("" if unknown)
}

#[derive(Serialize)]
struct GameState {
    hand_number: u32,
    phase: String,
    street: String,
    pot: usize,
    board: Vec<String>,
    hero: PlayerView,
    players: Vec<PlayerView>,
    legal_actions: Vec<String>,
    to_call: usize,
    min_raise: usize,
    max_bet: usize,
    dealer_seat: u8,
    sb_seat: u8,
    bb_seat: u8,
    small_blind: usize,
    big_blind: usize,
    session_over: bool,
    /// True when there is a within-hand human action that `undo_action()` can
    /// step back to (the undo stack is non-empty). Cleared each `next_hand()`.
    can_undo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_result: Option<Vec<PotResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    showdown: Option<Vec<ShowdownPlayer>>,
    forced_fold_count: u32,
    /// Live value of the `ADAPTIVE` engine flag (see `set_adaptive`). Surfaced so
    /// the UI/tests can confirm the persisted preference actually reached WASM —
    /// the toggle's checkbox state alone can silently diverge from the engine.
    adaptive: bool,
    /// Live value of the difficulty tier (see `set_difficulty`), same
    /// rationale as `adaptive`: the UI selector alone can silently diverge
    /// from the engine.
    difficulty: String,
    /// EPIC-49 Phase 3: per-seat session performance (net chips and chips/100
    /// over completed hands). Empty until a session exists; busted seats stay
    /// in the report with their full loss.
    session_report: Vec<SeatReport>,
}

/// One seat's session performance (EPIC-49 Phase 3). `chips_per_100` is the
/// arena-bench metric: net chips per 100 completed hands. Caveat carried from
/// the EPIC: tournament-style elimination biases chips/100 (busted seats stop
/// accumulating hands); treat cross-seat comparisons over long runs
/// accordingly.
#[derive(Serialize, Clone)]
struct SeatReport {
    seat: u8,
    name: String,
    net_chips: i64,
    hands_played: u32,
    chips_per_100: f64,
}

#[derive(Serialize)]
struct PlayerView {
    seat: u8,
    name: String,
    chips: usize,
    bet: usize,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hole_cards: Option<Vec<String>>,
    is_dealer: bool,
    is_sb: bool,
    is_bb: bool,
    /// EPIC-47 Phase 4 HUD. `None` until this seat's identity has at least one
    /// completed hand in the `StatsRegistry` (so absent at hand 1); the UI
    /// renders a VPIP/PFR/AF badge when present, dimmed while `confidence` is
    /// `"low"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<HudStats>,
}

/// Per-seat opponent-model summary surfaced to the HUD (EPIC-47 Phase 4).
/// `vpip`/`pfr` are fractions in `0.0..=1.0`; `af` is the postflop aggression
/// factor (bets+raises / calls), `None` when there were no postflop calls.
#[derive(Serialize)]
struct HudStats {
    vpip: Option<f64>,
    pfr: Option<f64>,
    af: Option<f64>,
    /// `"low"` (<50 hands) | `"medium"` (50–199) | `"high"` (200+).
    confidence: String,
    hands: u64,
}

// ── Bot stepping ──────────────────────────────────────────────────────────────

/// Run exactly one bot action and return a JSON description of what happened.
///
/// Returns `{"done":true}` when it is the human's turn or the hand is over.
/// Returns `{"done":false,"seat":N,"name":"…","action_label":"bets $300"}` otherwise.
/// JS calls this in a loop with a 1-second pause between calls to animate play.
#[wasm_bindgen]
pub fn step_bot() -> String {
    let phase = PHASE.with(|p| *p.borrow());
    if !matches!(phase, SessionPhase::BotsActing) {
        return serde_json::json!({"done": true}).to_string();
    }

    let next = SESSION.with(|s| s.borrow_mut().as_mut().and_then(|sess| sess.next_actor()));

    match next {
        None => {
            PHASE.with(|p| *p.borrow_mut() = SessionPhase::HandComplete);
            serde_json::json!({"done": true}).to_string()
        }
        Some(0) if !IS_ALL_BOT.with(|f| *f.borrow()) => {
            PHASE.with(|p| *p.borrow_mut() = SessionPhase::WaitingForHuman);
            serde_json::json!({"done": true}).to_string()
        }
        Some(seat) => {
            let all_bot = IS_ALL_BOT.with(|f| *f.borrow());
            let bot_idx = if all_bot {
                seat as usize
            } else {
                (seat as usize).saturating_sub(1)
            };

            let (action, call_amount, allin_chips, name, hole_cards) = SESSION.with(|s| {
                let session_ref = s.borrow();
                let Some(session) = session_ref.as_ref() else {
                    return (PlayerAction::Fold, 0usize, 0usize, String::new(), Vec::new());
                };
                let call_amt = session.table.to_call(seat);
                let chips = session
                    .table
                    .seats
                    .get_seat(seat)
                    .map_or(0, |s| s.player.chips);
                let name = session
                    .table
                    .seats
                    .get_seat(seat)
                    .map(|s| s.player.handle.clone())
                    .unwrap_or_default();
                let hole_cards: Vec<String> =
                    session
                        .table
                        .seats
                        .get_seat(seat)
                        .map_or_else(Vec::new, |s| {
                            sorted_hand(s.cards.as_slice())
                                .iter()
                                .map(card_to_str)
                                .collect()
                        });

                // Build the decider's snapshot WITH opponent stats attached
                // (EPIC-47 Phase 2). The snapshot borrows the registry, so it
                // must be built and consumed inside the REGISTRY borrow — hence
                // the nested scopes rather than moving the snapshot out to a
                // separate decide step. A bare `RuleBasedDecider` ignores the
                // stats; when adaptivity is on (Phase 3) the decider here is an
                // `ExploitativeDecider` wrapper that reads them and adjusts the
                // profile before deciding. Either way this call site is unchanged.
                let action = REGISTRY.with(|reg| {
                    BOTS.with(|b| {
                        RNG.with(|r| {
                            let registry = reg.borrow();
                            let snapshot = TableSnapshot::from_table_with_stats(
                                &session.table,
                                seat,
                                &registry,
                            );
                            let mut bots = b.borrow_mut();
                            let mut rng = r.borrow_mut();
                            bots.get_mut(bot_idx).map_or(PlayerAction::Fold, |bot| {
                                bot.decider
                                    .decide_seeded(&bot.profile, &snapshot, &mut *rng)
                            })
                        })
                    })
                });

                (action, call_amt, chips, name, hole_cards)
            });

            let attempted_label = action_label(&action, call_amount, allin_chips);
            let mut label = attempted_label.clone();
            let mut fallback_notice: Option<String> = None;

            let outcome = SESSION.with(|s| {
                s.borrow_mut()
                    .as_mut()
                    .map(|sess| apply_bot_action(sess, seat, &action))
                    .unwrap_or(ActionOutcome::ForcedFold {
                        rejected_err: "no active session".to_string(),
                    })
            });

            match outcome {
                ActionOutcome::Applied => {}
                ActionOutcome::Repaired { applied } => {
                    // A rejected bet/raise was clamped to a legal amount (or
                    // downgraded to call/check) so the bot keeps playing the
                    // hand instead of folding it — NOT a forced fold, so the
                    // counter stays reserved for genuinely unrepairable errors.
                    let applied_label = action_label(&applied, call_amount, allin_chips);
                    fallback_notice = Some(format!(
                        "adjusted {attempted_label} for {name} → {applied_label}"
                    ));
                    label = applied_label;
                }
                ActionOutcome::ForcedFold { rejected_err } => {
                    FORCED_FOLD_COUNT.with(|c| *c.borrow_mut() += 1);
                    console_warn(&format!(
                        "Bot action rejected at seat {seat}: attempted `{attempted_label}`; \
                         no legal repair, forcing fold ({rejected_err})"
                    ));
                    fallback_notice = Some(format!(
                        "engine rejected {attempted_label} for {name}; folded"
                    ));
                    label = "folds".to_string();
                }
            }

            serde_json::json!({
                "done": false,
                "seat": seat,
                "name": name,
                "action_label": label,
                "hole_cards": hole_cards,
                "fallback_notice": fallback_notice,
            })
            .to_string()
        }
    }
}

// ── State serialization ───────────────────────────────────────────────────────

fn build_game_state() -> String {
    let phase_val = PHASE.with(|p| *p.borrow());

    SESSION.with(|s| {
        let borrow = s.borrow();
        let Some(session) = borrow.as_ref() else {
            return serde_json::to_string(&GameState {
                hand_number: 0,
                phase: "Uninitialized".to_string(),
                street: "Preflop".to_string(),
                pot: 0,
                board: vec![],
                hero: empty_player_view(0),
                players: vec![],
                legal_actions: vec![],
                to_call: 0,
                min_raise: 0,
                max_bet: 0,
                dealer_seat: 0,
                sb_seat: 0,
                bb_seat: 0,
                small_blind: 0,
                big_blind: 0,
                session_over: false,
                can_undo: false,
                error: None,
                last_result: None,
                showdown: None,
                forced_fold_count: 0,
                adaptive: ADAPTIVE.with(|a| *a.borrow()),
                difficulty: DIFFICULTY.with(|d| d.borrow().as_str().to_string()),
                session_report: Vec::new(),
            })
            .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string());
        };

        let table = &session.table;
        let phase_str = match phase_val {
            SessionPhase::BotsActing => "BotsActing",
            SessionPhase::WaitingForHuman => "WaitingForHuman",
            SessionPhase::HandComplete => "HandComplete",
            SessionPhase::SessionOver => "SessionOver",
            SessionPhase::Uninitialized => "Uninitialized",
        };

        // A completed hand is a real showdown only when 2+ seats are still in
        // the hand (all-ins included); a river fold-out has exactly one.
        let is_showdown =
            phase_val == SessionPhase::HandComplete && table.seats.active_in_hand().len() >= 2;
        let street = street_from_board(table.board.len(), is_showdown);
        let board: Vec<String> = table.board.iter().map(card_to_str).collect();

        let dealer_seat = table.button;
        let sb_seat = table.determine_small_blind();
        let bb_seat = table.determine_big_blind();

        let to_call = table.to_call(0);
        // min_raise is the minimum *total* bet/raise-to amount.
        // Raise(n) validates n - table.bet >= min_raise_increment, so the
        // minimum valid total is table.bet + increment.  Bet on a fresh street
        // has table.bet == 0, so the formula still gives the right answer (1 BB).
        let min_raise = table.bet + table.min_raise();
        let hero_chips = table.seats.get_seat(0).map_or(0, |s| s.player.chips);
        let max_bet = hero_chips;

        let legal_actions = derive_legal_actions(to_call, hero_chips, table.bet);

        // Bot views — reveal hole cards at HandComplete/Showdown for in-hand bots.
        // In Arena (all-bot spectator) mode there is no one to hide from, so every
        // in-hand seat is face-up at all times; play mode reveals only at a
        // full-board hand end.
        let reveal_bot_cards = IS_ALL_BOT.with(|f| *f.borrow())
            || (phase_val == SessionPhase::HandComplete && table.board.len() == 5);

        // Attach EPIC-47 Phase 4 HUD stats from the session registry. Borrowed
        // once for hero + every bot view; a fresh session's registry is empty,
        // so `stats` stays `None` (no badges) until hands complete.
        let (hero_view, players) = REGISTRY.with(|reg| {
            let registry = reg.borrow();
            let reg_ref = Some(&*registry);

            // Hero view — always show hole cards.
            let hero_view =
                seat_to_player_view(table, 0, dealer_seat, sb_seat, bb_seat, true, reg_ref);

            let players: Vec<PlayerView> = (1..table.seats.0.len())
                .map(|i| {
                    let seat = i as u8;
                    let in_hand = table
                        .seats
                        .get_seat(seat)
                        .is_some_and(|s| is_in_hand(&s.player.state));
                    seat_to_player_view(
                        table,
                        seat,
                        dealer_seat,
                        sb_seat,
                        bb_seat,
                        reveal_bot_cards && in_hand,
                        reg_ref,
                    )
                })
                .collect();

            (hero_view, players)
        });

        // Consume any one-shot values so they surface to the UI exactly once.
        let last_error = LAST_ERROR.with(|e| e.borrow_mut().take());
        let last_result = LAST_HAND_RESULT.with(|r| r.borrow_mut().take());
        let showdown = LAST_SHOWDOWN.with(|r| r.borrow_mut().take());
        let forced_fold_count = FORCED_FOLD_COUNT.with(|c| *c.borrow());
        // Non-empty history = a human action this hand is available to undo.
        let can_undo = HISTORY.with(|h| !h.borrow().is_empty());

        // Completed hands for the chips/100 report: the current hand counts
        // once it has finished (HandComplete covers SessionOver's final hand;
        // next_hand() advances hand_number only when a new hand starts).
        let completed_hands = match phase_val {
            SessionPhase::HandComplete | SessionPhase::SessionOver => session.hand_number,
            _ => session.hand_number.saturating_sub(1),
        };

        let state = GameState {
            hand_number: session.hand_number,
            phase: phase_str.to_string(),
            street,
            pot: table.pot,
            board,
            hero: hero_view,
            players,
            legal_actions,
            to_call,
            min_raise,
            max_bet,
            dealer_seat,
            sb_seat,
            bb_seat,
            small_blind: table.forced.small_blind,
            big_blind: table.forced.big_blind,
            session_over: phase_val == SessionPhase::SessionOver,
            can_undo,
            error: last_error,
            last_result,
            showdown,
            forced_fold_count,
            adaptive: ADAPTIVE.with(|a| *a.borrow()),
            difficulty: DIFFICULTY.with(|d| d.borrow().as_str().to_string()),
            session_report: session_report(table, completed_hands),
        };

        serde_json::to_string(&state)
            .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string())
    })
}

/// Records the session's original seating (seat, name, starting chips) so the
/// chips/100 report can net every player — including busted seats the table
/// has since emptied (EPIC-49 Phase 3).
fn record_session_start(seats: &[Seat]) {
    let entries: Vec<(u8, String, usize)> = seats
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u8, s.player.handle.clone(), s.player.chips))
        .collect();
    SESSION_START_CHIPS.with(|c| *c.borrow_mut() = entries);
}

/// Builds the per-seat session report from original seating vs current
/// stacks. `completed_hands` is the number of finished hands; a seat that
/// busted keeps its full loss (current chips read as 0 once the seat empties).
#[allow(clippy::cast_precision_loss)]
fn session_report(table: &Table, completed_hands: u32) -> Vec<SeatReport> {
    SESSION_START_CHIPS.with(|c| {
        c.borrow()
            .iter()
            .map(|(seat, name, start)| {
                let current = table
                    .seats
                    .get_seat(*seat)
                    .filter(|s| !s.is_empty())
                    .map_or(0, |s| s.player.chips);
                let net_chips = current as i64 - *start as i64;
                let chips_per_100 = if completed_hands == 0 {
                    0.0
                } else {
                    net_chips as f64 * 100.0 / f64::from(completed_hands)
                };
                SeatReport {
                    seat: *seat,
                    name: name.clone(),
                    net_chips,
                    hands_played: completed_hands,
                    chips_per_100,
                }
            })
            .collect()
    })
}

/// Maps a `PlayerStats` entry to the HUD payload, or `None` when the seat's
/// identity has no completed hands yet (keeps the badge absent at hand 1).
fn hud_stats(s: &PlayerStats) -> Option<HudStats> {
    if s.hands_dealt == 0 {
        return None;
    }
    let confidence = match s.confidence() {
        Confidence::Low => "low",
        Confidence::Medium => "medium",
        Confidence::High => "high",
    };
    Some(HudStats {
        vpip: s.vpip(),
        pfr: s.pfr(),
        af: s.aggression_factor(),
        confidence: confidence.to_string(),
        hands: s.hands_dealt,
    })
}

fn seat_to_player_view(
    table: &Table,
    seat: u8,
    dealer_seat: u8,
    sb_seat: u8,
    bb_seat: u8,
    show_cards: bool,
    registry: Option<&StatsRegistry>,
) -> PlayerView {
    let Some(s) = table.seats.get_seat(seat) else {
        return empty_player_view(seat);
    };

    let hole_cards: Option<Vec<String>> = if show_cards {
        let cards: Vec<String> = sorted_hand(s.cards.as_slice())
            .iter()
            .map(card_to_str)
            .collect();
        if cards.is_empty() { None } else { Some(cards) }
    } else {
        // For bots when not at showdown, indicate cards are face-down (2 blanks).
        let dealt = s
            .cards
            .as_slice()
            .iter()
            .filter(|c| **c != Card::BLANK)
            .count();
        if dealt > 0 && is_in_hand(&s.player.state) {
            Some(vec!["__".to_string(); dealt])
        } else {
            None
        }
    };

    let stats = registry.and_then(|r| r.get(s.player.id)).and_then(hud_stats);

    PlayerView {
        seat,
        name: s.player.handle.clone(),
        chips: s.player.chips,
        bet: s.player.bet,
        state: state_to_str(&s.player.state),
        hole_cards,
        is_dealer: seat == dealer_seat,
        is_sb: seat == sb_seat,
        is_bb: seat == bb_seat,
        stats,
    }
}

fn empty_player_view(seat: u8) -> PlayerView {
    PlayerView {
        seat,
        name: String::new(),
        chips: 0,
        bet: 0,
        state: "Out".to_string(),
        hole_cards: None,
        is_dealer: false,
        is_sb: false,
        is_bb: false,
        stats: None,
    }
}

fn derive_legal_actions(to_call: usize, hero_chips: usize, current_bet: usize) -> Vec<String> {
    if hero_chips == 0 {
        return vec![];
    }
    if to_call == 0 {
        // No bet facing us.
        let mut actions = vec!["Check".to_string()];
        actions.push("Bet".to_string());
        actions.push("AllIn".to_string());
        actions
    } else {
        // There is a bet to call/raise.
        let mut actions = vec!["Fold".to_string()];
        // Only offer Call when the player can cover the full amount; when they
        // can't, AllIn is the correct action (calling for less / going all-in).
        if hero_chips >= to_call {
            actions.push("Call".to_string());
        }
        // Can raise only if chips exceed the call and exceed the current bet.
        if hero_chips > to_call && hero_chips > current_bet {
            actions.push("Raise".to_string());
        }
        actions.push("AllIn".to_string());
        actions
    }
}

/// Builds a seat's decider. `joker` seats morph each hand via `JokerDecider`;
/// everyone else plays `RuleBasedDecider`. When `adaptive` (EPIC-47 Phase 3),
/// the base decider is wrapped in `ExploitativeDecider` with the canonical
/// `ExploitConfig` so it deviates from its baseline profile using the largest
/// opponent's observed stats. The wrapper is a no-op until the registry has
/// enough hands to clear `ExploitConfig`'s min-hands gates, so early hands are
/// identical to the unwrapped path.
fn make_bot_seat(profile: BotProfile, adaptive: bool) -> BotSeat {
    let is_joker = profile.name == "joker";
    let decider: Box<dyn BotDecider> = match (is_joker, adaptive) {
        (true, false) => Box::new(JokerDecider::default()),
        (false, false) => Box::new(RuleBasedDecider),
        (true, true) => Box::new(ExploitativeDecider::wrap_with_config(
            JokerDecider::default(),
            ExploitConfig::default(),
        )),
        (false, true) => Box::new(ExploitativeDecider::wrap_with_config(
            RuleBasedDecider,
            ExploitConfig::default(),
        )),
    };
    BotSeat { profile, decider }
}

fn notify_bots_new_hand() {
    RNG.with(|r| {
        let mut rng = r.borrow_mut();
        BOTS.with(|b| {
            for bot in b.borrow_mut().iter_mut() {
                bot.decider.on_new_hand_with_rng(&mut *rng);
            }
        });
    });
}

fn action_label(action: &PlayerAction, call_amount: usize, allin_chips: usize) -> String {
    match action {
        PlayerAction::Fold => "folds".to_string(),
        PlayerAction::Check => "checks".to_string(),
        PlayerAction::Call => format!("calls ${call_amount}"),
        PlayerAction::Bet(n) => format!("bets ${n}"),
        PlayerAction::Raise(n) => format!("raises to ${n}"),
        PlayerAction::AllIn => format!("goes all-in ${allin_chips}"),
    }
}

/// Result of feeding a bot's chosen action to the engine.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum ActionOutcome {
    /// The bot's action was legal and applied as-is.
    Applied,
    /// The bot's action was rejected but repaired to a legal alternative
    /// (a min-sized bet/raise, or a call/check/all-in) which was applied.
    Repaired { applied: PlayerAction },
    /// No legal alternative applied; the bot was force-folded. This is the
    /// genuine "safety net fired" signal that `FORCED_FOLD_COUNT` tracks.
    ForcedFold { rejected_err: String },
}

/// Applies a bot's chosen `action`, repairing engine-rejected actions instead
/// of silently folding wherever possible.
///
/// `RuleBasedDecider` routinely sizes a raise below the NLHE minimum increment
/// (rejected as `InsufficientIncrement`). Rather than discard the hand the bot
/// wanted to raise, we walk an escalating ladder that preserves as much of the
/// aggressive intent as the engine allows, and only fold as a last resort:
///
/// 1. clamp an under-sized `Bet`/`Raise` up to `min_raise_to()` (same variant);
/// 2. call the outstanding bet (or check if there is none);
/// 3. go all-in (covers the short stack that cannot afford a min-raise or call);
/// 4. fold.
///
/// Each candidate is validated by the engine; a rejected candidate is a no-op
/// (validation precedes mutation in pkcore), so the ladder is safe to try in
/// sequence. Only step 4 counts as a forced fold.
fn apply_bot_action(session: &mut PokerSession, seat: u8, action: &PlayerAction) -> ActionOutcome {
    match session.apply_action(seat, *action) {
        Ok(()) => ActionOutcome::Applied,
        Err(e) => {
            let rejected_err = e.to_string();

            // 1. Clamp an under-sized bet/raise to the minimum legal amount,
            //    preserving the original variant.
            let min_to = session.table.min_raise_to();
            let mut ladder: Vec<PlayerAction> = match action {
                PlayerAction::Bet(_) => vec![PlayerAction::Bet(min_to)],
                PlayerAction::Raise(_) => vec![PlayerAction::Raise(min_to)],
                _ => Vec::new(),
            };
            // 2. Passive continuation: call if facing a bet, else check.
            if session.table.to_call(seat) > 0 {
                ladder.push(PlayerAction::Call);
            } else {
                ladder.push(PlayerAction::Check);
            }
            // 3. Short-stack continuation (can't cover a min-raise or the call).
            ladder.push(PlayerAction::AllIn);

            for candidate in ladder {
                if session.apply_action(seat, candidate).is_ok() {
                    return ActionOutcome::Repaired { applied: candidate };
                }
            }

            // 4. Last resort — fold is legal for any player facing action.
            let _ = session.apply_action(seat, PlayerAction::Fold);
            ActionOutcome::ForcedFold { rejected_err }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn console_warn(msg: &str) {
    web_sys::console::warn_1(&JsValue::from_str(msg));
}

#[cfg(not(target_arch = "wasm32"))]
fn console_warn(_msg: &str) {}

/// Embedded standard bot lineup (EPIC-49 Phase 1), generated from
/// `builtin_standard_pool()` (see `generate_standard_bundle`).
static BUNDLE_STANDARD: &str = include_str!("../data/bots/standard.yaml");

/// Parses the embedded standard lineup into a shuffleable profile pool. Falls
/// back to the built-in pool if the YAML is ever unparseable — a bad edit must
/// never brick the game. (The bundle is also parse-checked at test time by
/// `standard_bundle_matches_default_pool`.)
fn standard_profiles() -> Vec<BotProfile> {
    match serde_yaml_bw::from_str::<BotBundle>(BUNDLE_STANDARD) {
        Ok(bundle) => bundle.profiles,
        Err(e) => {
            console_warn(&format!(
                "bot lineup YAML failed to parse ({e}); using built-in defaults"
            ));
            builtin_standard_pool()
        }
    }
}

/// Embedded weak bot lineup (EPIC-49 Phase 3), generated from
/// `builtin_weak_pool()` (see `generate_weak_bundle`).
static BUNDLE_WEAK: &str = include_str!("../data/bots/weak.yaml");

/// Parses the embedded weak lineup, falling back to the built-in weak pool on
/// parse failure (same never-brick contract as `standard_profiles`).
fn weak_profiles() -> Vec<BotProfile> {
    match serde_yaml_bw::from_str::<BotBundle>(BUNDLE_WEAK) {
        Ok(bundle) => bundle.profiles,
        Err(e) => {
            console_warn(&format!(
                "weak lineup YAML failed to parse ({e}); using built-in weak pool"
            ));
            builtin_weak_pool()
        }
    }
}

/// Embedded strong bot lineup (EPIC-49 Phase 3), generated from
/// `builtin_strong_pool()` (see `generate_strong_bundle`).
static BUNDLE_STRONG: &str = include_str!("../data/bots/strong.yaml");

/// Parses the embedded strong lineup, falling back to the built-in strong
/// pool on parse failure (same never-brick contract as `standard_profiles`).
fn strong_profiles() -> Vec<BotProfile> {
    match serde_yaml_bw::from_str::<BotBundle>(BUNDLE_STRONG) {
        Ok(bundle) => bundle.profiles,
        Err(e) => {
            console_warn(&format!(
                "strong lineup YAML failed to parse ({e}); using built-in strong pool"
            ));
            builtin_strong_pool()
        }
    }
}

/// The profile pool for a difficulty tier.
fn profiles_for(difficulty: Difficulty) -> Vec<BotProfile> {
    match difficulty {
        Difficulty::Weak => weak_profiles(),
        Difficulty::Standard => standard_profiles(),
        Difficulty::Strong => strong_profiles(),
    }
}

/// Whether the next lineup build wraps deciders in `ExploitativeDecider`:
/// weak never adapts (beginner-friendly); standard and strong honor the
/// Settings toggle. Adaptation is deliberately NOT the strong tier's lever:
/// the matchup bench measured it as a chips/100 drag in long bot-vs-bot runs
/// (its value is modeling *human* tendencies, which a bot bench can't see) —
/// the strong tier's lever is its sharpened bundle (`strengthen`).
fn effective_adaptive(difficulty: Difficulty) -> bool {
    match difficulty {
        Difficulty::Weak => false,
        Difficulty::Standard | Difficulty::Strong => ADAPTIVE.with(|a| *a.borrow()),
    }
}

// ── EPIC-49 Phase 3: the weak tier's dampened pool ────────────────────────────

/// The weak-tier pool: every standard archetype pushed toward the
/// loose-passive corner by `weaken`. The joker is deliberately absent —
/// `JokerDecider` morphs into full-strength `default_profiles()` each hand,
/// which would leak standard play into the weak tier.
fn builtin_weak_pool() -> Vec<BotProfile> {
    BotProfile::default_profiles()
        .into_iter()
        .map(weaken)
        .collect()
}

/// Dampens a profile into its weak-tier form: the spewy beginner who bluffs
/// far too often and never extracts value. Its chips flow out through
/// frequent small bluffs into opponents who call correctly (the one steady
/// profile-driven outflow this decider has), while its made hands earn
/// almost nothing: `value_threshold` is pushed near 1.0 so it stops
/// value-betting, aggression is floored so it stops raising for value, and
/// it is position-blind (playbook stripped). Hand selection keeps the
/// archetype's own preflop range so each personality stays recognizable.
///
/// Tuning provenance (measured, not vibes — see `difficulty_ordering_tests`):
/// a wide-range "plays any two" fish gambles too much (all-in variance
/// drowns the ordering signal), and an over-tight nit actually *profits*
/// against these over-bluffing archetypes. Over-bluffing + no value is the
/// form that loses reliably.
fn weaken(mut profile: BotProfile) -> BotProfile {
    use pkcore::analysis::gto::solver_config::BetSize;
    use pkcore::bot::betting_strategy::BettingStrategy;

    let mut strategy = BettingStrategy::new(5, 40, 2, vec![BetSize::half_pot()]);
    strategy.value_threshold = Some(0.97);
    profile.betting_strategy = strategy;
    profile.playbook = None;
    profile
}

// ── EPIC-49 Phase 3: the strong tier's disciplined pool ───────────────────────

/// The strong-tier pool: every standard archetype sharpened by `strengthen`.
/// Like the weak pool, the joker is absent — it morphs into standard-strength
/// profiles, which would dilute the strong tier.
fn builtin_strong_pool() -> Vec<BotProfile> {
    builtin_standard_pool()
        .into_iter()
        .filter(|p| p.name != "joker")
        .map(strengthen)
        .map(|p| with_decision(p, strong_decision()))
        .collect()
}

/// Sharpens a profile into its strong-tier form: the disciplined regular.
/// Hand selection tightens to a solid ~10% opening range (premiums get paid
/// in this engine; junk cannot back up its postflop equity), bluffing — the
/// one steady profile-driven chip *leak* this decider has (see `weaken`) —
/// is clamped hard, and the value-bet threshold drops so made hands charge
/// worse ones. Aggression keeps each archetype's own values and playbook
/// position grades — a measured decision: lifting aggression across the
/// board bought all-in variance, not edge. The same discipline is applied at
/// the flat baseline and at every playbook position, since the decider
/// resolves positional strategy first. As of EPIC-50 this is the profile-tuning
/// layer *beneath* the strong tier's real capability knobs (`decision.equity =
/// fast{500}` + `ranges: position_aware`, pkcore 0.3.0 via `strong_decision()`),
/// not a substitute for them — the bench shows equity roughly triples the strong
/// edge (≈+24k → +67k chips/100).
/// Validated by the matchup harness (`difficulty_ordering_tests`), not
/// vibes.
fn strengthen(mut profile: BotProfile) -> BotProfile {
    use pkcore::bot::betting_strategy::BettingStrategy;
    use pkcore::bot::playbook::{Playbook, PlaybookEntry};
    use pkcore::bot::positional_betting::PositionalBetting;
    use pkcore::bot::range_strategy::RangeStrategy;
    use pkcore::casino::position::Position;

    fn discipline(s: &BettingStrategy) -> BettingStrategy {
        let mut d = BettingStrategy::new(
            s.aggression_factor.value(),
            s.bluff_frequency.value().min(8),
            s.check_raise_frequency.value(),
            s.preferred_bet_sizes.clone(),
        );
        d.value_threshold = Some(0.5);
        d
    }

    let ranges = &profile.range_strategy;
    profile.range_strategy = RangeStrategy::new(
        "44+, AJ+, KQ, KJs", // ~top 10% — pkcore's PERCENT_10 reference range
        ranges.three_bet.clone(),
        ranges.call_three_bet.clone(),
        ranges.postflop_cbet_frequency.value(),
    );

    profile.betting_strategy = discipline(&profile.betting_strategy);
    if let Some(playbook) = profile.playbook.take() {
        let mut sharpened = Playbook::new();
        for seats in [6u8, 9u8] {
            let Some(entry) = playbook.for_seats(seats) else {
                continue;
            };
            let positions: &[Position] = if seats == 6 {
                &[
                    Position::LJ,
                    Position::HJ,
                    Position::CO,
                    Position::BTN,
                    Position::SB,
                    Position::BB,
                ]
            } else {
                &[
                    Position::UTG,
                    Position::UTGP1,
                    Position::EP,
                    Position::LJ,
                    Position::HJ,
                    Position::CO,
                    Position::BTN,
                    Position::SB,
                    Position::BB,
                ]
            };
            let mut betting = PositionalBetting::new(profile.betting_strategy.clone());
            for &pos in positions {
                betting.insert(pos, discipline(entry.positional_betting.for_position(pos)));
            }
            sharpened.insert(
                seats,
                PlaybookEntry::new(entry.position_ranges.clone(), betting),
            );
        }
        profile.playbook = Some(sharpened);
    }
    profile
}

// ── EPIC-49 Phase 2: position awareness for every archetype ───────────────────

/// The built-in pool with position awareness completed: `default_profiles()` +
/// `joker()`, with playbooks attached to the five archetypes pkcore ships flat
/// (EPIC-49 Phase 2). Single source of truth for the standard bundle — the
/// YAML generator serializes exactly this, the parity gate compares against
/// it, and the runtime fallback returns it. The joker stays playbook-free:
/// `JokerDecider` ignores its own profile and morphs into a (playbook-bearing)
/// standard profile each hand.
fn builtin_standard_pool() -> Vec<BotProfile> {
    let mut profiles: Vec<BotProfile> = BotProfile::default_profiles()
        .into_iter()
        .map(attach_archetype_playbook)
        .map(|p| with_decision(p, standard_decision()))
        .collect();
    // The joker keeps the default (off) decision block: JokerDecider ignores
    // its own profile and morphs into a pkcore default profile each hand, so a
    // decision block here would never be consulted.
    profiles.push(BotProfile::joker());
    profiles
}

// ── EPIC-50: graded decision-capability knobs per tier ────────────────────────

/// Attaches a pkcore `DecisionConfig` to a profile so the graded capability
/// knobs (real equity, position-aware ranges, pot-odds discipline) travel with
/// the bundle YAML. `#[serde(default)]` upstream means a default block is
/// omitted from the YAML, so bundles that don't opt in are byte-identical.
fn with_decision(mut profile: BotProfile, decision: DecisionConfig) -> BotProfile {
    profile.decision = decision;
    profile
}

/// Standard-tier knobs: activate the position-aware ranges that
/// `attach_archetype_playbook` already carries dormant. Equity is left at the
/// proxy and pot-odds at the strict default — the real MC engine is the *strong*
/// tier's lever, kept off the default in-browser path so standard play stays
/// cheap (one `compute()` per postflop decision is reserved for players who opt
/// into the strong difficulty).
fn standard_decision() -> DecisionConfig {
    DecisionConfig {
        ranges: RangeMode::PositionAware,
        ..DecisionConfig::default()
    }
}

/// Strong-tier knobs: real multi-way equity plus position-aware ranges, layered
/// on the `strengthen()` base (tighter range, bluff clamp, `value_threshold`).
/// 500 Monte Carlo samples = the EPIC-48 Phase-0 browser budget (2.8 ms HU /
/// 5.7 ms 4-way). Pot-odds discipline stays at the strict default (1.0);
/// `exploit` stays `off` (a bot-vs-bot drag — EPIC-49 corrigendum §1 keeps
/// adaptation a user toggle, not a tier lever).
fn strong_decision() -> DecisionConfig {
    DecisionConfig {
        equity: EquityMode::Fast { samples: 500 },
        ranges: RangeMode::PositionAware,
        ..DecisionConfig::default()
    }
}

/// Attaches an authored [`Playbook`] to the five archetypes pkcore ships
/// without one, and re-grades the two whose pkcore playbooks are positionally
/// flat (tight_passive at both sizes, loose_aggressive at 9-max); gto — whose
/// pkcore playbook is already fully graded — and joker pass through unchanged.
///
/// Design note: the decider consults `betting_for(seats, pos)` today, so the
/// positional *betting* grades below change live behavior; the positional
/// *ranges* are carried as data for upstream pkcore EPIC-36 (`ranges:
/// position_aware`), which is why each archetype reuses the closest existing
/// pkcore range chart rather than authoring new ones (upstreaming candidate).
fn attach_archetype_playbook(profile: BotProfile) -> BotProfile {
    use pkcore::bot::playbook::{Playbook, PlaybookEntry};
    use pkcore::bot::position_ranges::PositionRanges;
    use pkcore::casino::position::Position::{BB, BTN, CO, EP, HJ, LJ, SB, UTG, UTGP1};

    // Positional aggression grades, anchored at each archetype's flat baseline
    // (EP below it, BTN above it) so the style is preserved while position
    // discipline emerges: (position, aggression, bluff, check_raise).
    type Grade = (pkcore::casino::position::Position, u8, u8, u8);
    let (six, nine): (&[Grade], &[Grade]) = match profile.name.as_str() {
        // Baseline 70/20/15 — selective but forceful; opens up on the button.
        "tight_aggressive" => (
            &[
                (LJ, 60, 15, 12),
                (HJ, 64, 17, 13),
                (CO, 68, 19, 14),
                (BTN, 78, 24, 18),
                (SB, 68, 18, 14),
                (BB, 66, 18, 16),
            ],
            &[
                (UTG, 55, 12, 10),
                (UTGP1, 57, 13, 11),
                (EP, 60, 15, 12),
                (LJ, 62, 16, 12),
                (HJ, 65, 17, 13),
                (CO, 70, 19, 15),
                (BTN, 78, 24, 18),
                (SB, 66, 17, 13),
                (BB, 66, 18, 16),
            ],
        ),
        // Baseline 15/3/2 — passive everywhere, faint button uptick.
        "loose_passive" => (
            &[
                (LJ, 12, 2, 2),
                (HJ, 13, 2, 2),
                (CO, 14, 3, 2),
                (BTN, 20, 5, 3),
                (SB, 14, 3, 2),
                (BB, 13, 3, 2),
            ],
            &[
                (UTG, 10, 2, 1),
                (UTGP1, 10, 2, 1),
                (EP, 11, 2, 2),
                (LJ, 12, 2, 2),
                (HJ, 13, 3, 2),
                (CO, 15, 3, 2),
                (BTN, 20, 5, 3),
                (SB, 13, 3, 2),
                (BB, 13, 3, 2),
            ],
        ),
        // Baseline 90/55/30 — relentless, but even a maniac fears the gun.
        "maniac" => (
            &[
                (LJ, 84, 48, 26),
                (HJ, 86, 50, 27),
                (CO, 88, 52, 28),
                (BTN, 97, 62, 34),
                (SB, 90, 55, 30),
                (BB, 88, 54, 32),
            ],
            &[
                (UTG, 80, 45, 24),
                (UTGP1, 82, 46, 25),
                (EP, 84, 48, 26),
                (LJ, 85, 49, 26),
                (HJ, 87, 51, 27),
                (CO, 90, 54, 29),
                (BTN, 97, 62, 34),
                (SB, 89, 53, 28),
                (BB, 88, 54, 32),
            ],
        ),
        // Baseline 65/0/5 — by-the-book position discipline, still zero bluffs.
        "abc" => (
            &[
                (LJ, 55, 0, 4),
                (HJ, 58, 0, 4),
                (CO, 62, 0, 5),
                (BTN, 72, 0, 6),
                (SB, 60, 0, 5),
                (BB, 60, 0, 5),
            ],
            &[
                (UTG, 48, 0, 3),
                (UTGP1, 50, 0, 3),
                (EP, 52, 0, 4),
                (LJ, 55, 0, 4),
                (HJ, 58, 0, 4),
                (CO, 63, 0, 5),
                (BTN, 72, 0, 6),
                (SB, 58, 0, 4),
                (BB, 60, 0, 5),
            ],
        ),
        // Baseline 95/45/40 — push-or-fold with tighter early-position shoves.
        "short_stack_ninja" => (
            &[
                (LJ, 88, 38, 34),
                (HJ, 90, 40, 36),
                (CO, 93, 43, 38),
                (BTN, 100, 52, 44),
                (SB, 95, 45, 40),
                (BB, 94, 46, 42),
            ],
            &[
                (UTG, 82, 34, 30),
                (UTGP1, 84, 36, 32),
                (EP, 86, 38, 33),
                (LJ, 88, 39, 34),
                (HJ, 90, 41, 36),
                (CO, 93, 44, 38),
                (BTN, 100, 52, 44),
                (SB, 94, 44, 39),
                (BB, 94, 46, 42),
            ],
        ),
        // pkcore's own tight_passive/loose_aggressive playbooks carry flat
        // (ungraded) positional betting for some table sizes — graded here so
        // "every archetype plays position-differentiated poker" holds at both
        // 6-max and the 9-max tables this app actually deals.
        // Baseline 25/5/3 — pkcore ships it flat at BOTH sizes.
        "tight_passive" => (
            &[
                (LJ, 21, 4, 3),
                (HJ, 22, 4, 3),
                (CO, 24, 5, 3),
                (BTN, 32, 8, 5),
                (SB, 24, 5, 3),
                (BB, 23, 5, 4),
            ],
            &[
                (UTG, 18, 3, 2),
                (UTGP1, 19, 3, 2),
                (EP, 20, 4, 2),
                (LJ, 21, 4, 3),
                (HJ, 22, 4, 3),
                (CO, 24, 5, 3),
                (BTN, 32, 8, 5),
                (SB, 24, 5, 3),
                (BB, 23, 5, 4),
            ],
        ),
        // Baseline 75/35/20 — pkcore grades 6-max but ships 9-max flat; the
        // 6-max grades mirror pkcore's own (loose_aggressive_six_max).
        "loose_aggressive" => (
            &[
                (LJ, 65, 30, 18),
                (HJ, 68, 33, 20),
                (CO, 72, 36, 22),
                (BTN, 80, 40, 25),
                (SB, 70, 35, 20),
                (BB, 68, 33, 25),
            ],
            &[
                (UTG, 66, 28, 16),
                (UTGP1, 68, 29, 17),
                (EP, 70, 31, 18),
                (LJ, 71, 32, 18),
                (HJ, 73, 33, 19),
                (CO, 76, 36, 20),
                (BTN, 84, 42, 24),
                (SB, 74, 34, 19),
                (BB, 73, 34, 21),
            ],
        ),
        // gto's playbook is already graded at both sizes; joker's profile is
        // never consulted by its decider.
        _ => return profile,
    };

    // Closest existing pkcore range chart per style (data for EPIC-36; the
    // decider does not consult positional ranges yet).
    let (ranges_six, ranges_nine) = match profile.name.as_str() {
        // Same 6-max charts pkcore's own playbooks pair with these styles.
        "loose_passive" | "maniac" | "loose_aggressive" => (
            PositionRanges::loose_aggressive_six_max(),
            PositionRanges::gto_nine_max(),
        ),
        "short_stack_ninja" | "tight_passive" => (
            PositionRanges::tight_passive_six_max(),
            PositionRanges::gto_nine_max(),
        ),
        _ => (PositionRanges::gto_six_max(), PositionRanges::gto_nine_max()),
    };

    let mut playbook = Playbook::new();
    playbook.insert(
        6,
        PlaybookEntry::new(ranges_six, graded_betting(&profile.betting_strategy, six)),
    );
    playbook.insert(
        9,
        PlaybookEntry::new(ranges_nine, graded_betting(&profile.betting_strategy, nine)),
    );
    profile.with_playbook(playbook)
}

/// Builds a [`PositionalBetting`] whose default is the archetype's flat
/// baseline and whose per-position entries apply the given aggression grades,
/// keeping the archetype's preferred bet sizes at every position.
fn graded_betting(
    base: &pkcore::bot::betting_strategy::BettingStrategy,
    grades: &[(pkcore::casino::position::Position, u8, u8, u8)],
) -> pkcore::bot::positional_betting::PositionalBetting {
    use pkcore::bot::betting_strategy::BettingStrategy;
    use pkcore::bot::positional_betting::PositionalBetting;

    let mut betting = PositionalBetting::new(base.clone());
    for &(pos, aggression, bluff, check_raise) in grades {
        betting.insert(
            pos,
            BettingStrategy::new(
                aggression,
                bluff,
                check_raise,
                base.preferred_bet_sizes.clone(),
            ),
        );
    }
    betting
}

fn street_from_board(board_len: usize, is_showdown: bool) -> String {
    match board_len {
        0 => "Preflop",
        3 => "Flop",
        4 => "Turn",
        // A full board is a showdown only when 2+ players revealed; a river
        // fold-out (or mid-hand) reads "River".
        _ => {
            if is_showdown {
                "Showdown"
            } else {
                "River"
            }
        }
    }
    .to_string()
}

fn state_to_str(state: &PlayerState) -> String {
    match state {
        PlayerState::Out | PlayerState::Ready => "Out",
        PlayerState::YetToAct
        | PlayerState::Check
        | PlayerState::Call(_)
        | PlayerState::Blind(_)
        | PlayerState::Bet(_)
        | PlayerState::Raise(_)
        | PlayerState::ReRaise(_)
        | PlayerState::Showdown(_) => "Active",
        PlayerState::AllIn(_) => "AllIn",
        PlayerState::Fold => "Fold",
    }
    .to_string()
}

fn is_in_hand(state: &PlayerState) -> bool {
    !matches!(
        state,
        PlayerState::Out | PlayerState::Ready | PlayerState::Fold
    )
}

/// Non-blank cards ordered high rank first (poker display convention).
/// Relies on pkcore's derived `Card: Ord` (rank-primary in the Cactus-Kev
/// u32); a descending sort is Ace-high first. Suit is a minor tiebreak.
fn sorted_hand(cards: &[Card]) -> Vec<Card> {
    let mut v: Vec<Card> = cards
        .iter()
        .copied()
        .filter(|c| *c != Card::BLANK)
        .collect();
    v.sort_unstable_by(|a, b| b.cmp(a)); // descending: Ace-high first
    v
}

fn card_to_str(card: &Card) -> String {
    let rank = card.get_rank().to_char();
    let suit = match card.get_suit() {
        Suit::SPADES => 's',
        Suit::HEARTS => 'h',
        Suit::DIAMONDS => 'd',
        Suit::CLUBS => 'c',
        _ => '_',
    };
    format!("{rank}{suit}")
}

fn error_state(msg: &str) -> String {
    serde_json::json!({
        "phase": "Error",
        "error": msg,
        "session_over": false
    })
    .to_string()
}

fn hand_rank_name_to_str(name: HandRankName) -> Option<String> {
    match name {
        HandRankName::StraightFlush => Some("Straight Flush".to_string()),
        HandRankName::FourOfAKind => Some("Four of a Kind".to_string()),
        HandRankName::FullHouse => Some("Full House".to_string()),
        HandRankName::Flush => Some("Flush".to_string()),
        HandRankName::Straight => Some("Straight".to_string()),
        HandRankName::ThreeOfAKind => Some("Three of a Kind".to_string()),
        HandRankName::TwoPair => Some("Two Pair".to_string()),
        HandRankName::Pair => Some("Pair".to_string()),
        HandRankName::HighCard => Some("High Card".to_string()),
        HandRankName::RazzLow => Some("Razz Low".to_string()),
        HandRankName::Invalid => None,
    }
}

#[cfg(test)]
mod street_tests {
    use super::street_from_board;

    #[test]
    fn full_board_is_showdown_only_when_contested() {
        // Genuine showdown: 5-card board, 2+ still in.
        assert_eq!(street_from_board(5, true), "Showdown");
        // River fold-out: 5-card board, only one left -> not a showdown.
        assert_eq!(street_from_board(5, false), "River");
    }

    #[test]
    fn pre_river_streets_ignore_showdown_flag() {
        assert_eq!(street_from_board(0, false), "Preflop");
        assert_eq!(street_from_board(3, false), "Flop");
        assert_eq!(street_from_board(4, false), "Turn");
        // Flag is irrelevant before the river.
        assert_eq!(street_from_board(3, true), "Flop");
    }
}

#[cfg(test)]
mod sort_tests {
    use super::{card_to_str, sorted_hand};
    use pkcore::card::Card;
    use std::str::FromStr;

    fn codes(cards: &[Card]) -> Vec<String> {
        sorted_hand(cards).iter().map(card_to_str).collect()
    }

    #[test]
    fn orders_high_rank_first_and_drops_blank() {
        let hand = [
            Card::from_str("2c").unwrap(),
            Card::from_str("As").unwrap(),
            Card::BLANK, // padding must be dropped
            Card::from_str("Td").unwrap(),
            Card::from_str("Kh").unwrap(),
        ];
        assert_eq!(codes(&hand), vec!["As", "Kh", "Td", "2c"]);
    }

    #[test]
    fn already_sorted_stays_sorted() {
        let hand = [Card::from_str("Ah").unwrap(), Card::from_str("Kd").unwrap()];
        assert_eq!(codes(&hand), vec!["Ah", "Kd"]);
    }
}

#[cfg(test)]
mod decider_path_parity_tests {
    use super::*;
    use std::collections::BTreeSet;

    fn hero_action(session: &PokerSession) -> PlayerAction {
        let to_call = session.table.to_call(0);
        let chips = session
            .table
            .seats
            .get_seat(0)
            .map_or(0, |s| s.player.chips);
        if to_call == 0 {
            PlayerAction::Check
        } else if chips >= to_call {
            PlayerAction::Call
        } else {
            PlayerAction::AllIn
        }
    }

    /// Regression guard for EPIC-46 Phase 1b: the web loop replaced
    /// `BotProfile::decide(&table, seat, rng)` with
    /// `RuleBasedDecider::decide_seeded(&profile, &TableSnapshot::from_table(..), rng)`.
    /// The refactor is only safe if those two paths are *behaviourally
    /// identical* for a non-joker seat.
    ///
    /// We cannot compare two independently dealt games: `PokerSession::start_hand`
    /// reshuffles via `Cards::shuffle_in_place`, which draws from the entropy
    /// thread-local RNG (`pkcore .../cards.rs`), not our seeded `SmallRng` — so two
    /// runs are dealt different boards and their action sequences only match by
    /// luck. Instead we drive a *single* game and, at every bot decision, evaluate
    /// **both** paths against the identical `table` + a *clone* of the live RNG.
    /// Because the deck never enters the assertion, the check is deal-independent
    /// and cannot flake; it fails only if pkcore's convenience method and the
    /// explicit decider path genuinely diverge (e.g. a future `decide` that builds
    /// a stats-bearing snapshot — the exact seam EPIC-47 will touch).
    #[test]
    fn convenience_and_decider_paths_agree_at_every_decision() {
        let profile = BotProfile::default_profiles()
            .into_iter()
            .find(|p| p.name != "joker")
            .expect("expected at least one non-joker profile");

        let seats = Seats::new(vec![
            Seat::new(Player::new_with_chips("You".to_string(), 10_000)),
            Seat::new(Player::new_with_chips(profile.name.clone(), 10_000)),
        ]);
        let table = Table::nlh_from_seats(seats, ForcedBets::new(50, 100));
        let mut session = PokerSession::new(table);
        session.start_hand().expect("failed to start first hand");

        let mut rng = SmallRng::seed_from_u64(42);
        let rule = RuleBasedDecider;
        let mut compared = 0usize;
        let mut hands_completed = 0usize;

        while hands_completed < 8 {
            match session.next_actor() {
                None => {
                    session.end_hand().expect("failed to end hand");
                    session.eliminate_busted();
                    if session.count_funded() < 2 {
                        break;
                    }
                    session.table.button_up();
                    session.start_hand().expect("failed to start next hand");
                    hands_completed += 1;
                }
                Some(0) => {
                    let action = hero_action(&session);
                    session
                        .apply_action(0, action)
                        .expect("hero action should always apply");
                }
                Some(1) => {
                    // Old path (convenience method) and new path (explicit
                    // decider) evaluated on the SAME state with clones of the
                    // live RNG, so neither consumes the stream before the other.
                    let mut rng_convenience = rng.clone();
                    let via_convenience =
                        profile.decide(&session.table, 1, &mut rng_convenience);

                    let mut rng_decider = rng.clone();
                    let snapshot = TableSnapshot::from_table(&session.table, 1);
                    let via_decider =
                        rule.decide_seeded(&profile, &snapshot, &mut rng_decider);

                    assert_eq!(
                        via_convenience, via_decider,
                        "decider path diverged from convenience method at decision {compared}"
                    );

                    // Advance the real RNG identically to either path and apply.
                    rng = rng_convenience;
                    compared += 1;
                    // Mirror production's `step_bot` fallback: some deals produce
                    // an action the engine rejects (e.g. a raise below the minimum
                    // increment), which the web loop force-converts to a Fold. We
                    // do the same so the game keeps advancing deterministically —
                    // the invariant under test is the assert_eq above, not that
                    // every decider action is legal.
                    if session.apply_action(1, via_convenience).is_err() {
                        session
                            .apply_action(1, PlayerAction::Fold)
                            .expect("forced fold should always apply");
                    }
                }
                Some(other) => panic!("unexpected seat in two-player test: {other}"),
            }
        }

        assert!(
            compared > 0,
            "test exercised no bot decisions — game never reached the bot seat"
        );
    }

    /// EPIC-46 acceptance #2: "the joker demonstrably plays different styles
    /// across hands." `JokerDecider::on_new_hand_with_rng` re-rolls its active
    /// profile from `BotProfile::default_profiles()` each hand; the web loop
    /// fires it via `notify_bots_new_hand()`. Here we fire the same hook over N
    /// seeded hands and confirm the joker adopts at least two distinct
    /// aggression profiles. The joker's active profile is private, but its
    /// `Debug` impl exposes the active profile's name, which we map back to an
    /// aggression factor. Seeded RNG ⇒ fully deterministic, never flaky.
    #[test]
    fn joker_morphs_style_across_hands() {
        let profiles = BotProfile::default_profiles();
        let joker = JokerDecider::new_with_rng(&mut SmallRng::seed_from_u64(7));
        let mut rng = SmallRng::seed_from_u64(7);

        let mut aggression_factors = BTreeSet::new();
        for _ in 0..40 {
            joker.on_new_hand_with_rng(&mut rng);
            // Debug renders `JokerDecider { active: "<name>" }`.
            let dbg = format!("{joker:?}");
            let active = profiles
                .iter()
                .find(|p| dbg.contains(&format!("\"{}\"", p.name)))
                .expect("joker's active profile should be one of the default profiles");
            aggression_factors.insert(active.betting_strategy.aggression_factor);
        }

        assert!(
            aggression_factors.len() >= 2,
            "joker should exhibit at least two distinct aggression profiles across \
             hands, but only saw {aggression_factors:?}"
        );
    }
}

#[cfg(test)]
mod repair_ladder_tests {
    use super::*;

    /// A heads-up 50/100 NL session advanced to the first preflop decision, so
    /// the actor faces an outstanding bet and a voluntary raise is legal. This
    /// is the exact state that provokes `RuleBasedDecider`'s under-sized raise
    /// in production (see `docs/EPIC-46_Decider_Integration.md`).
    fn heads_up_at_first_action() -> (PokerSession, u8) {
        let seats = Seats::new(vec![
            Seat::new(Player::new_with_chips("A".to_string(), 10_000)),
            Seat::new(Player::new_with_chips("B".to_string(), 10_000)),
        ]);
        let table = Table::nlh_from_seats(seats, ForcedBets::new(50, 100));
        let mut session = PokerSession::new(table);
        session.start_hand().expect("failed to start hand");
        let seat = session.next_actor().expect("a seat should be to act preflop");
        (session, seat)
    }

    /// EPIC-46 (repair ladder): a raise sized below the NLHE minimum increment
    /// is `InsufficientIncrement`-rejected by the engine. Rather than discard the
    /// bot's aggressive intent with a fold, `apply_bot_action` clamps it *up* to
    /// `min_raise_to()` and applies that, reporting `Repaired` — so the raise
    /// still happens and `FORCED_FOLD_COUNT` is not touched.
    #[test]
    fn undersized_raise_is_clamped_up_to_the_minimum() {
        let (mut session, seat) = heads_up_at_first_action();
        let min_to = session.table.min_raise_to();
        // One chip under the minimum legal raise-to: a legal-*intent* raise with
        // an illegal *amount* — the category-3 rejection the ladder exists for.
        //
        // If the rejected raise were *not* a true no-op (pkcore validates before
        // it mutates), the clamp candidate below would see a shifted
        // `min_raise_to()` and this assertion's expected amount would be wrong —
        // so this test also guards that invariant.
        let undersized = PlayerAction::Raise(min_to - 1);

        let outcome = apply_bot_action(&mut session, seat, &undersized);
        assert_eq!(
            outcome,
            ActionOutcome::Repaired {
                applied: PlayerAction::Raise(min_to)
            },
            "under-sized raise should clamp up to Raise(min_raise_to), not fold"
        );
    }

    /// A legal action passes straight through as `Applied` and is unmodified —
    /// the ladder must never perturb a bot whose action the engine accepts.
    #[test]
    fn legal_action_applies_unchanged() {
        let (mut session, seat) = heads_up_at_first_action();
        let outcome = apply_bot_action(&mut session, seat, &PlayerAction::Call);
        assert_eq!(outcome, ActionOutcome::Applied);
    }
}

#[cfg(test)]
mod stats_plumbing_tests {
    use super::*;

    /// EPIC-47 Phase 1 acceptance 1d. Drives real hands, ingesting each into a
    /// `StatsRegistry` keyed by the seats' real `Player` Uuids exactly as
    /// production `next_hand()` will (5-tuple snapshot +
    /// `from_table_state_with_ids` + `ingest_hand`). Asserts the registry
    /// correlated every identity. We assert plumbing invariants, NOT VPIP
    /// values: `PokerSession::start_hand` reshuffles from the entropy RNG (not
    /// our seeded `SmallRng`), so specific magnitudes would flake — the same
    /// deal-independence rationale as `convenience_and_decider_paths_agree_*`.
    #[test]
    fn stats_registry_correlates_players_by_identity() {
        let profile = BotProfile::default_profiles()
            .into_iter()
            .find(|p| p.name != "joker")
            .expect("expected at least one non-joker profile");

        let seats = Seats::new(vec![
            Seat::new(Player::new_with_chips("You".to_string(), 10_000)),
            Seat::new(Player::new_with_chips(profile.name.clone(), 10_000)),
        ]);
        let table = Table::nlh_from_seats(seats, ForcedBets::new(50, 100));
        let mut session = PokerSession::new(table);
        session.start_hand().expect("failed to start first hand");

        // Capture ids once; they must stay stable across every hand.
        let hero_id = session.table.seats.0[0].player.id;
        let bot_id = session.table.seats.0[1].player.id;
        assert_ne!(hero_id, bot_id, "distinct seats must have distinct ids");
        assert!(
            !hero_id.is_nil() && !bot_id.is_nil(),
            "real players must have non-nil ids"
        );

        let mut registry = StatsRegistry::new();
        let mut rng = SmallRng::seed_from_u64(42);
        let rule = RuleBasedDecider;
        let mut hands_completed = 0usize;

        while hands_completed < 6 {
            match session.next_actor() {
                None => {
                    // Mirror production next_hand(): snapshot BEFORE end_hand,
                    // build an id-threaded HandHistory, ingest, then advance.
                    let event_log = session.table.event_log.clone();
                    let button = session.table.button;
                    let snapshot: Vec<PlayerSnapshot> = session
                        .table
                        .seats
                        .0
                        .iter()
                        .enumerate()
                        .filter_map(|(i, seat)| {
                            if seat.is_empty() {
                                return None;
                            }
                            Some((
                                i as u8,
                                seat.player.handle.clone(),
                                10_000,
                                None,
                                Some(seat.player.id),
                            ))
                        })
                        .collect();

                    session.end_hand().expect("failed to end hand");

                    let ending: Vec<(u8, usize)> = session
                        .table
                        .seats
                        .0
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| !s.is_empty())
                        .map(|(i, s)| (i as u8, s.player.chips))
                        .collect();

                    let hh = HandHistory::from_table_state_with_ids(
                        hands_completed,
                        0,
                        button,
                        &ForcedBets::new(50, 100),
                        &snapshot,
                        "",
                        &Winnings::default(),
                        &event_log,
                        &ending,
                        "test",
                        None,
                    );
                    registry.ingest_hand(&hh);

                    session.eliminate_busted();
                    if session.count_funded() < 2 {
                        break;
                    }
                    session.table.button_up();
                    session.start_hand().expect("failed to start next hand");
                    hands_completed += 1;
                }
                Some(0) => {
                    // Minimal legal hero action (inlined — the sibling test
                    // module's `hero_action` helper is module-private).
                    let to_call = session.table.to_call(0);
                    let chips = session
                        .table
                        .seats
                        .get_seat(0)
                        .map_or(0, |s| s.player.chips);
                    let action = if to_call == 0 {
                        PlayerAction::Check
                    } else if chips >= to_call {
                        PlayerAction::Call
                    } else {
                        PlayerAction::AllIn
                    };
                    session
                        .apply_action(0, action)
                        .expect("hero action should always apply");
                }
                Some(1) => {
                    let snapshot = TableSnapshot::from_table(&session.table, 1);
                    let action = rule.decide_seeded(&profile, &snapshot, &mut rng);
                    if session.apply_action(1, action).is_err() {
                        session
                            .apply_action(1, PlayerAction::Fold)
                            .expect("forced fold should always apply");
                    }
                }
                Some(other) => panic!("unexpected seat in two-player test: {other}"),
            }
        }

        // Ids stayed stable across every hand.
        assert_eq!(
            session.table.seats.0[0].player.id, hero_id,
            "hero id changed across hands"
        );
        assert_eq!(
            session.table.seats.0[1].player.id, bot_id,
            "bot id changed across hands"
        );

        // Plumbing invariant: the registry correlated each identity.
        assert!(
            registry.get(hero_id).is_some(),
            "registry should have stats for the hero id"
        );
        let bot_stats = registry
            .get(bot_id)
            .expect("registry should have stats for the bot id");
        if let Some(v) = bot_stats.vpip() {
            assert!((0.0..=1.0).contains(&v), "vpip out of range: {v}");
        }
    }
}

#[cfg(test)]
mod stats_injection_tests {
    use super::*;

    /// EPIC-47 Phase 2 acceptance 2b: making stats *reachable* by the decider
    /// (production `step_bot` now builds `TableSnapshot::from_table_with_stats`
    /// instead of `from_table`) must NOT change what `RuleBasedDecider` does —
    /// the shipped decider ignores `opponent_stats`; adaptation is Phase 3.
    ///
    /// We prove it directly: with a *populated* registry attached, the decider's
    /// action on identical state (and a clone of the same RNG) is byte-identical
    /// to the no-stats path. Single state + cloned RNG ⇒ deal-independent, never
    /// flaky. pkcore locks this upstream via
    /// `rule_based_decider_ignores_opponent_stats`; this guards *our* seam — that
    /// we pass `&bot.profile` unchanged and never accidentally wrap the decider.
    #[test]
    fn stats_bearing_snapshot_does_not_change_rule_based_action() {
        let profile = BotProfile::default_profiles()
            .into_iter()
            .find(|p| p.name != "joker")
            .expect("expected at least one non-joker profile");

        let seats = Seats::new(vec![
            Seat::new(Player::new_with_chips("You".to_string(), 10_000)),
            Seat::new(Player::new_with_chips(profile.name.clone(), 10_000)),
        ]);
        let table = Table::nlh_from_seats(seats, ForcedBets::new(50, 100));
        let mut session = PokerSession::new(table);
        session.start_hand().expect("failed to start first hand");

        let hero_id = session.table.seats.0[0].player.id;
        let bot_id = session.table.seats.0[1].player.id;

        // Populate the registry so the stats path is genuinely non-empty for
        // both seats — an empty registry would make the assertion vacuous.
        let mut registry = StatsRegistry::new();
        let synthetic: Vec<PlayerSnapshot> = vec![
            (0, "You".to_string(), 10_000, None, Some(hero_id)),
            (1, profile.name.clone(), 10_000, None, Some(bot_id)),
        ];
        let hh = HandHistory::from_table_state_with_ids(
            0,
            0,
            0,
            &ForcedBets::new(50, 100),
            &synthetic,
            "",
            &Winnings::default(),
            &[],
            &[(0, 10_000), (1, 10_000)],
            "test",
            None,
        );
        registry.ingest_hand(&hh);
        assert!(
            registry.get(bot_id).is_some(),
            "registry must be populated for the bot for a meaningful comparison"
        );

        // Advance to the bot's (seat 1) first decision.
        loop {
            match session.next_actor() {
                Some(1) => break,
                Some(0) => {
                    let to_call = session.table.to_call(0);
                    let chips = session
                        .table
                        .seats
                        .get_seat(0)
                        .map_or(0, |s| s.player.chips);
                    let action = if to_call == 0 {
                        PlayerAction::Check
                    } else if chips >= to_call {
                        PlayerAction::Call
                    } else {
                        PlayerAction::AllIn
                    };
                    session
                        .apply_action(0, action)
                        .expect("hero action should always apply");
                }
                other => panic!("expected to reach the bot's decision, got {other:?}"),
            }
        }

        let rule = RuleBasedDecider;
        let snap_plain = TableSnapshot::from_table(&session.table, 1);
        let snap_stats = TableSnapshot::from_table_with_stats(&session.table, 1, &registry);
        assert!(
            snap_stats.opponent_stats.is_some(),
            "from_table_with_stats should attach the registry borrow"
        );

        let base = SmallRng::seed_from_u64(99);
        let action_plain = rule.decide_seeded(&profile, &snap_plain, &mut base.clone());
        let action_stats = rule.decide_seeded(&profile, &snap_stats, &mut base.clone());

        assert_eq!(
            action_plain, action_stats,
            "attaching opponent stats changed RuleBasedDecider's action — the \
             injection seam must be behavior-neutral until Phase 3"
        );
    }
}

#[cfg(test)]
mod adaptive_wrapping_tests {
    use super::*;
    use pkcore::bot::exploit::{ExploitConfig, adjust_profile};
    use pkcore::bot::table_snapshot::SeatInfo;
    use pkcore::games::GamePhase;
    use pkcore::games::betting_structure::{BetTier, BettingStructure};

    /// Builds a `StatsRegistry` in which `opp_id` reads as a loose-passive
    /// calling station (VPIP high, PFR zero) by ingesting one authored hand:
    /// heads-up, the villain limp-calls preflop. That is enough for the
    /// EPIC-27 station rule (`vpip > threshold && pfr < passive_threshold`) to
    /// fire once the min-hands gate is dropped — the rule rewrites
    /// `preferred_bet_sizes`, so the adjusted profile is materially different
    /// regardless of the baseline's `bluff_frequency`.
    fn station_registry(opp_id: uuid::Uuid, bot_id: uuid::Uuid) -> StatsRegistry {
        // Villain = seat 0 (button/SB, acts first heads-up), Hero = seat 1 (BB).
        let event_log = vec![
            TableAction::ForcedBetSmallBlind(0, 50),
            TableAction::ForcedBetBigBlind(1, 100),
            TableAction::Call(0, 100), // villain voluntarily puts in chips → VPIP, no raise → PFR 0
            TableAction::Check(1),     // hero checks the option
        ];
        let snapshot: Vec<PlayerSnapshot> = vec![
            (0, "Villain".to_string(), 10_000, None, Some(opp_id)),
            (1, "Hero".to_string(), 10_000, None, Some(bot_id)),
        ];
        let hh = HandHistory::from_table_state_with_ids(
            0,
            0,
            0,
            &ForcedBets::new(50, 100),
            &snapshot,
            "",
            &Winnings::default(),
            &event_log,
            &[(0, 9_900), (1, 10_000)],
            "test",
            None,
        );
        let mut registry = StatsRegistry::new();
        registry.ingest_hand(&hh);
        registry
    }

    /// A fixed flop decision point for the bot (seat 1), first to act with the
    /// option to bet or check. Deal-independent: every field is authored, so
    /// the RNG seed is the only source of variation — no entropy shuffle, no
    /// flakiness. `opponent_stats` is attached iff `registry` is `Some`.
    fn flop_snapshot<'a>(
        opp_id: uuid::Uuid,
        bot_id: uuid::Uuid,
        registry: Option<&'a StatsRegistry>,
    ) -> TableSnapshot<'a> {
        TableSnapshot {
            seat: 1,
            phase: GamePhase::Flop,
            board: "Ks 7h 2c".parse().expect("valid board"),
            hole_cards: "Ad Kd".parse().expect("valid hole cards"),
            pot: 300,
            to_call: 0,
            current_bet: 0,
            min_raise: 100,
            my_chips: 9_800,
            stacks: vec![
                SeatInfo {
                    id: opp_id,
                    seat: 0,
                    name: "Villain".to_string(),
                    chips: 9_800,
                    bet: 0,
                    is_active: true,
                },
                SeatInfo {
                    id: bot_id,
                    seat: 1,
                    name: "Hero".to_string(),
                    chips: 9_800,
                    bet: 0,
                    is_active: true,
                },
            ],
            big_blind: 100,
            betting_structure: BettingStructure::NoLimit,
            bet_tier: BetTier::Small,
            checked_this_street: false,
            dealer_button: Some(0),
            seat_count: 2,
            logical_seat: Some(1),
            opponent_stats: registry,
        }
    }

    /// An `ExploitConfig` whose min-hands gates are dropped to 1 so the single
    /// ingested hand clears them. Thresholds are the canonical defaults — the
    /// authored villain (VPIP 1.0, PFR 0.0) genuinely crosses the
    /// calling-station and loose-passive lines; only the sample-size gate is
    /// relaxed for the unit test.
    fn open_gate_config() -> ExploitConfig {
        ExploitConfig {
            min_hands_light: 1,
            min_hands_heavy: 1,
            ..ExploitConfig::default()
        }
    }

    /// EPIC-47 Phase 3 acceptance 3c. With adaptivity on (an `ExploitativeDecider`
    /// wrapper, exactly what `make_bot_seat(_, true)` builds) and the opponent's
    /// stats past the gate, at least one decision differs from the unwrapped
    /// baseline; with no stats attached the wrapper is a byte-for-byte no-op.
    #[test]
    fn adaptive_wrapping_diverges_after_gate_and_is_neutral_without_stats() {
        let opp_id = Player::new_with_chips("Villain".to_string(), 10_000).id;
        let bot_id = Player::new_with_chips("Hero".to_string(), 10_000).id;
        let registry = station_registry(opp_id, bot_id);

        // Sanity: the authored villain really is a loose-passive station.
        let stats = registry.get(opp_id).expect("villain must be tracked");
        assert_eq!(stats.vpip(), Some(1.0), "villain limp-called → VPIP 1.0");
        assert_eq!(stats.pfr(), Some(0.0), "villain never raised → PFR 0.0");

        let cfg = open_gate_config();
        let snap_stats = flop_snapshot(opp_id, bot_id, Some(&registry));

        // Pick a profile the station rule actually moves (its baseline
        // preferred_bet_sizes differ from the rule's value bet sizing).
        let profile = BotProfile::default_profiles()
            .into_iter()
            .find(|p| adjust_profile(p, &snap_stats, &cfg) != *p)
            .expect("at least one default profile must be exploitably adjusted");
        let adjusted = adjust_profile(&profile, &snap_stats, &cfg);
        assert_ne!(
            adjusted, profile,
            "the exploit config must materially adjust the chosen profile"
        );

        let bare = RuleBasedDecider;
        let wrapped = ExploitativeDecider::wrap_with_config(RuleBasedDecider, cfg.clone());
        let empty = StatsRegistry::new();
        let snap_no_stats = flop_snapshot(opp_id, bot_id, Some(&empty));

        let mut diverged = false;
        for seed in 0u64..256 {
            // Routing: the wrapper feeds the ADJUSTED profile to the inner
            // decider — its action equals the bare decider on the adjusted
            // profile, on identical state and an identically-seeded RNG.
            let via_wrapper =
                wrapped.decide_seeded(&profile, &snap_stats, &mut SmallRng::seed_from_u64(seed));
            let via_adjusted =
                bare.decide_seeded(&adjusted, &snap_stats, &mut SmallRng::seed_from_u64(seed));
            assert_eq!(
                via_wrapper, via_adjusted,
                "wrapper must decide via the stat-adjusted profile (seed {seed})"
            );

            // Divergence: adapted decision vs the unwrapped baseline.
            let baseline =
                bare.decide_seeded(&profile, &snap_stats, &mut SmallRng::seed_from_u64(seed));
            if via_wrapper != baseline {
                diverged = true;
            }

            // Neutrality: with the gate un-cleared (empty registry) the wrapper
            // is indistinguishable from the bare decider.
            let neutral =
                wrapped.decide_seeded(&profile, &snap_no_stats, &mut SmallRng::seed_from_u64(seed));
            let neutral_baseline =
                bare.decide_seeded(&profile, &snap_no_stats, &mut SmallRng::seed_from_u64(seed));
            assert_eq!(
                neutral, neutral_baseline,
                "adaptivity must be a no-op when no opponent stats are present (seed {seed})"
            );
        }

        assert!(
            diverged,
            "adaptation on must change at least one decision once the opponent's \
             stats clear the min-hands gate"
        );
    }
}

#[cfg(test)]
mod bot_bundle_fixture {
    use super::*;

    /// Fixture generator (run on demand): writes `data/bots/standard.yaml` from
    /// the built-in pool — `default_profiles()` + `joker()` with every
    /// archetype's playbook attached (EPIC-49 Phase 2) — so the embedded
    /// bundle is, by construction, identical to the code pool. Re-run with:
    ///   cargo test --lib generate_standard_bundle -- --ignored --nocapture
    #[test]
    #[ignore = "fixture generator; run explicitly to regenerate data/bots/standard.yaml"]
    fn generate_standard_bundle() {
        let bundle = BotBundle {
            name: "standard".to_string(),
            profiles: builtin_standard_pool(),
        };
        let yaml = serde_yaml_bw::to_string(&bundle).expect("serialize bundle");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/bots/standard.yaml");
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/data/bots"))
            .expect("create data/bots");
        std::fs::write(path, yaml).expect("write standard.yaml");
        eprintln!("wrote {path}");
    }

    /// EPIC-49 Phase 1 acceptance 1c + 1d (updated for Phase 2). Validates that
    /// the embedded bundle parses, and proves lineup parity: the YAML
    /// round-trips to *exactly* the code pool (`builtin_standard_pool()`), so
    /// the YAML and the runtime fallback can never drift apart. `BotProfile:
    /// PartialEq`, so this compares every range/betting/playbook field, not
    /// just names.
    #[test]
    fn standard_bundle_matches_default_pool() {
        let parsed = standard_profiles();
        let expected = builtin_standard_pool();

        assert_eq!(
            parsed.len(),
            expected.len(),
            "embedded lineup should carry all {} profiles",
            expected.len()
        );
        assert_eq!(
            parsed, expected,
            "embedded standard.yaml diverged from builtin_standard_pool(); \
             regenerate with `cargo test --lib generate_standard_bundle -- --ignored`"
        );

        // The joker must survive the round-trip by name so `make_bot_seat` still
        // routes it to `JokerDecider`.
        assert!(
            parsed.iter().any(|p| p.name == "joker"),
            "joker profile missing from embedded lineup"
        );

        // EPIC-50: every non-joker standard profile carries the standard-tier
        // decision knobs (real fast equity, position-aware ranges, moderate
        // pot-odds discipline). The joker keeps the default (off) block because
        // JokerDecider morphs into pkcore default profiles each hand.
        for p in &parsed {
            if p.name == "joker" {
                assert!(p.decision.is_default(), "joker should keep the default decision block");
            } else {
                assert_eq!(
                    p.decision,
                    standard_decision(),
                    "{} should carry the standard-tier decision knobs",
                    p.name
                );
            }
        }
    }

    /// The fallback path must itself be sound: if the embedded YAML were
    /// unparseable, `standard_profiles` still yields the full built-in pool.
    #[test]
    fn unparseable_bundle_falls_back_to_defaults() {
        // Exercises the fallback branch's construction directly (we can't easily
        // corrupt the `include_str!`ed const at runtime, so this guards the
        // else-arm's shape stays in sync with the default pool).
        assert!(serde_yaml_bw::from_str::<BotBundle>("not: [valid").is_err());
        assert_eq!(builtin_standard_pool().len(), 9);
    }

    /// Fixture generator (run on demand): writes `data/bots/weak.yaml` from
    /// the weak-tier pool (EPIC-49 Phase 3). Re-run with:
    ///   cargo test --lib generate_weak_bundle -- --ignored --nocapture
    #[test]
    #[ignore = "fixture generator; run explicitly to regenerate data/bots/weak.yaml"]
    fn generate_weak_bundle() {
        let bundle = BotBundle {
            name: "weak".to_string(),
            profiles: builtin_weak_pool(),
        };
        let yaml = serde_yaml_bw::to_string(&bundle).expect("serialize bundle");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/bots/weak.yaml");
        std::fs::write(path, yaml).expect("write weak.yaml");
        eprintln!("wrote {path}");
    }

    /// EPIC-49 Phase 3: the embedded weak bundle parses and round-trips to
    /// exactly `builtin_weak_pool()` — same drift gate as the standard bundle.
    #[test]
    fn weak_bundle_matches_weakened_pool() {
        let parsed = weak_profiles();
        let expected = builtin_weak_pool();

        assert_eq!(
            parsed.len(),
            expected.len(),
            "embedded weak lineup should carry all {} profiles",
            expected.len()
        );
        assert_eq!(
            parsed, expected,
            "embedded weak.yaml diverged from builtin_weak_pool(); regenerate \
             with `cargo test --lib generate_weak_bundle -- --ignored`"
        );

        // The joker must NOT be in the weak pool: its decider morphs into
        // full-strength default profiles, which would leak standard play
        // into the weak tier.
        assert!(
            parsed.iter().all(|p| p.name != "joker"),
            "joker must not appear in the weak lineup"
        );
        // And every profile is genuinely dampened and position-blind.
        for p in &parsed {
            assert!(
                p.playbook.is_none(),
                "{} should be position-blind in the weak tier",
                p.name
            );
            assert!(
                p.betting_strategy.aggression_factor < 26,
                "{} aggression should be capped in the weak tier",
                p.name
            );
        }
    }

    /// Fixture generator (run on demand): writes `data/bots/strong.yaml` from
    /// the strong-tier pool (EPIC-49 Phase 3). Re-run with:
    ///   cargo test --lib generate_strong_bundle -- --ignored --nocapture
    #[test]
    #[ignore = "fixture generator; run explicitly to regenerate data/bots/strong.yaml"]
    fn generate_strong_bundle() {
        let bundle = BotBundle {
            name: "strong".to_string(),
            profiles: builtin_strong_pool(),
        };
        let yaml = serde_yaml_bw::to_string(&bundle).expect("serialize bundle");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/bots/strong.yaml");
        std::fs::write(path, yaml).expect("write strong.yaml");
        eprintln!("wrote {path}");
    }

    /// EPIC-49 Phase 3: the embedded strong bundle parses and round-trips to
    /// exactly `builtin_strong_pool()` — same drift gate as the other bundles.
    #[test]
    fn strong_bundle_matches_strengthened_pool() {
        let parsed = strong_profiles();
        let expected = builtin_strong_pool();

        assert_eq!(
            parsed.len(),
            expected.len(),
            "embedded strong lineup should carry all {} profiles",
            expected.len()
        );
        assert_eq!(
            parsed, expected,
            "embedded strong.yaml diverged from builtin_strong_pool(); regenerate \
             with `cargo test --lib generate_strong_bundle -- --ignored`"
        );

        // Same joker rationale as the weak pool: it morphs into
        // standard-strength profiles, diluting the tier.
        assert!(
            parsed.iter().all(|p| p.name != "joker"),
            "joker must not appear in the strong lineup"
        );
        // Every profile is disciplined, and position awareness survives the
        // sharpening (BTN still out-aggresses UTG at 9-max).
        use pkcore::casino::position::Position;
        for p in &parsed {
            assert!(
                p.betting_strategy.bluff_frequency < 9,
                "{} bluff frequency should be clamped in the strong tier",
                p.name
            );
            assert!(
                p.playbook.is_some(),
                "{} should stay position-aware in the strong tier",
                p.name
            );
            let btn = p.betting_for(9, Position::BTN).aggression_factor;
            let utg = p.betting_for(9, Position::UTG).aggression_factor;
            assert!(
                btn > utg,
                "{}: strong-tier BTN aggression ({btn:?}) should still exceed UTG ({utg:?})",
                p.name
            );
            // EPIC-50: strong profiles carry the strict decision knobs.
            assert_eq!(
                p.decision,
                strong_decision(),
                "{} should carry the strong-tier decision knobs",
                p.name
            );
        }
    }

    /// EPIC-49 Phase 3: tier plumbing. `set_difficulty` gates both the pool
    /// choice and the effective adaptivity; unknown levels are rejected.
    #[test]
    fn difficulty_selects_pool_and_gates_adaptivity() {
        // Weak: dampened pool, adaptation off regardless of the toggle.
        assert!(set_difficulty("weak"));
        assert_eq!(difficulty_level(), "weak");
        set_adaptive(true);
        assert!(!effective_adaptive(Difficulty::Weak));
        assert_eq!(profiles_for(Difficulty::Weak).len(), 8);

        // Standard: standard pool, adaptation honors the toggle.
        assert!(set_difficulty("standard"));
        set_adaptive(false);
        assert!(!effective_adaptive(Difficulty::Standard));
        set_adaptive(true);
        assert!(effective_adaptive(Difficulty::Standard));
        assert_eq!(profiles_for(Difficulty::Standard).len(), 9);

        // Strong: sharpened pool; adaptation honors the toggle (measured as a
        // bot-vs-bot drag, so it is not forced — see effective_adaptive).
        assert!(set_difficulty("strong"));
        set_adaptive(false);
        assert!(!effective_adaptive(Difficulty::Strong));
        set_adaptive(true);
        assert!(effective_adaptive(Difficulty::Strong));
        assert_eq!(profiles_for(Difficulty::Strong).len(), 8);

        // Unknown levels are rejected and the current tier survives.
        assert!(!set_difficulty("nightmare"));
        assert_eq!(difficulty_level(), "strong");

        // Restore defaults for other tests sharing the thread-locals.
        set_difficulty("standard");
        set_adaptive(true);
    }
}

#[cfg(test)]
mod difficulty_ordering_tests {
    use super::*;

    /// One seat in a matchup: a profile, its decider (bare or adaptive), and
    /// which side of the comparison it plays for.
    struct MatchupSeat {
        profile: BotProfile,
        decider: Box<dyn BotDecider>,
        group_a: bool,
    }

    fn bare(profile: BotProfile, group_a: bool) -> MatchupSeat {
        MatchupSeat {
            profile,
            decider: Box::new(RuleBasedDecider),
            group_a,
        }
    }

    /// Kept (unused) as the probe that measured adaptation's bot-vs-bot
    /// value: seat `adaptive(p)` vs `bare(p)` to reproduce the −2.7k/−3.8k
    /// chips/100 drag recorded in `strong_tier_beats_standard_tier`'s doc
    /// comment and the EPIC-49 corrigendum.
    #[allow(dead_code)]
    fn adaptive(profile: BotProfile, group_a: bool) -> MatchupSeat {
        MatchupSeat {
            profile,
            decider: Box::new(ExploitativeDecider::wrap_with_config(
                RuleBasedDecider,
                ExploitConfig::default(),
            )),
            group_a,
        }
    }

    /// Fixed-stack cash-game matchup bench (EPIC-49 Phase 3 acceptance, the
    /// browser analogue of upstream EPIC-36's `SimTable` bench, using the
    /// cash-mode reset that EPIC itself plans). Plays `hands` hands at fixed
    /// 50/100 blinds and 100 BB stacks; after every hand each seat's result
    /// is banked into its net and its stack resets to 100 BB, so every hand
    /// is played under identical conditions. The reset is load-bearing: a
    /// refill-only-when-short variant let winners' stacks grow without bound,
    /// and the game's character drifted with depth — 12k-hand and 96k-hand
    /// runs of the same matchup produced opposite signs. Opponent stats are
    /// ingested per hand exactly as production `next_hand()` does, with
    /// stable player identities across the whole run so adaptive seats can
    /// clear `ExploitConfig`'s 30/50-hand gates.
    ///
    /// Returns (group A net chips, group B net chips). Chips are conserved,
    /// so the two nets sum to ~0 (exactly 0 barring pkcore's known multiway
    /// audit edge case, on which the hand's ingest is skipped).
    ///
    /// NOTE: pkcore's `start_hand` shuffles from the entropy RNG (no seeded
    /// deck exists), so this bench is statistical, not seed-reproducible —
    /// assertions must hold with margin over enough hands, the same
    /// deal-independence constraint every other test in this crate documents.
    fn run_matchup(mut seats: Vec<MatchupSeat>, hands: usize) -> (i64, i64) {
        const STACK: usize = 10_000;

        let seats_vec: Vec<Seat> = seats
            .iter()
            .enumerate()
            .map(|(i, m)| {
                Seat::new(Player::new_with_chips(
                    format!("{}#{i}", m.profile.name),
                    STACK,
                ))
            })
            .collect();
        let ids: Vec<uuid::Uuid> = seats_vec.iter().map(|s| s.player.id).collect();
        let names: Vec<String> = seats_vec.iter().map(|s| s.player.handle.clone()).collect();
        let table = Table::nlh_from_seats(Seats::new(seats_vec), ForcedBets::new(50, 100));
        let mut session = PokerSession::new(table);

        let mut registry = StatsRegistry::new();
        let mut rng = SmallRng::seed_from_u64(0xEC49);
        let mut nets = vec![0i64; seats.len()];

        for hand_num in 0..hands {
            session.start_hand().expect("start hand");

            while let Some(seat) = session.next_actor() {
                let action = {
                    let snapshot = TableSnapshot::from_table_with_stats(
                        &session.table,
                        seat,
                        &registry,
                    );
                    let m = &mut seats[seat as usize];
                    m.decider
                        .decide_seeded(&m.profile, &snapshot, &mut rng)
                };
                // Same escalating repair ladder as production step_bot().
                apply_bot_action(&mut session, seat, &action);
            }

            // Mirror production next_hand(): snapshot BEFORE end_hand, build an
            // id-threaded HandHistory, ingest into the registry.
            let event_log = session.table.event_log.clone();
            let button = session.table.button;
            let snapshot: Vec<PlayerSnapshot> = ids
                .iter()
                .zip(&names)
                .enumerate()
                .map(|(i, (id, name))| (i as u8, name.clone(), STACK, None, Some(*id)))
                .collect();

            if session.end_hand().is_ok() {
                let ending: Vec<(u8, usize)> = session
                    .table
                    .seats
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i as u8, s.player.chips))
                    .collect();
                let hh = HandHistory::from_table_state_with_ids(
                    hand_num,
                    0,
                    button,
                    &ForcedBets::new(50, 100),
                    &snapshot,
                    "",
                    &Winnings::default(),
                    &event_log,
                    &ending,
                    "bench",
                    None,
                );
                registry.ingest_hand(&hh);
            }
            session.table.event_log.clear();
            session.table.button_up();

            // Cash-mode reset: bank the hand's result, restore 100 BB.
            for (i, net) in nets.iter_mut().enumerate() {
                let seat = &mut session.table.seats.0[i];
                *net += seat.player.chips as i64 - STACK as i64;
                seat.player.chips = STACK;
            }

            // New-hand hook (the joker never plays here, but keep parity with
            // production's notify_bots_new_hand()).
            for m in &mut seats {
                m.decider.on_new_hand_with_rng(&mut rng);
            }
        }

        let mut net_a = 0i64;
        let mut net_b = 0i64;
        for (i, m) in seats.iter().enumerate() {
            if m.group_a {
                net_a += nets[i];
            } else {
                net_b += nets[i];
            }
        }
        (net_a, net_b)
    }

    /// Four solid archetypes benched on both sides of each matchup, so only the
    /// tier lever under test differs. EPIC-50: each side is drawn from its
    /// *actual* bundle pool (so the `decision:` knobs are exercised — standard's
    /// `ranges: position_aware`, strong's `equity: fast` on top of `strengthen`),
    /// not reconstructed from `strengthen`/`weaken` alone.
    const CORE_ARCHETYPES: [&str; 4] = ["gto", "tight_aggressive", "loose_aggressive", "abc"];

    /// Pulls a named archetype from a tier's pool (its full tier form, knobs
    /// included).
    fn pool_profile(pool: &[BotProfile], name: &str) -> BotProfile {
        pool.iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{name} missing from tier pool"))
            .clone()
    }

    /// EPIC-49 Phase 3 / EPIC-50 acceptance: the standard tier beats the weak
    /// tier. Both sides are drawn from their real bundle pools (so standard
    /// carries `ranges: position_aware`), alternating same-archetype seats to
    /// balance position.
    ///
    /// Statistical bench, run via `make bench-tiers` (release mode; ~65 s —
    /// proxy-equity on both sides, so no MC cost): measured edge
    /// **+23,836 chips/100** over 12,000 hands (EPIC-50, 2026-07-17), in line
    /// with the pre-knob ≈+22k. Entropy-dealt (see `run_matchup` note), hence
    /// not part of the default fast suite.
    #[test]
    #[ignore = "statistical bench (entropy-dealt); run via `make bench-tiers`"]
    fn standard_tier_beats_weak_tier() {
        let standard = builtin_standard_pool();
        let weak = builtin_weak_pool();
        let mut seats = Vec::new();
        for name in CORE_ARCHETYPES {
            seats.push(bare(pool_profile(&standard, name), true)); // standard tier
            seats.push(bare(pool_profile(&weak, name), false)); // weak tier
        }
        let hands = 12_000;
        let (standard_net, weak_net) = run_matchup(seats, hands);
        eprintln!(
            "standard {standard_net:+} vs weak {weak_net:+} over {hands} hands \
             ({:+.0} chips/100 differential)",
            (standard_net - weak_net) as f64 * 100.0 / hands as f64
        );
        assert!(
            standard_net > weak_net,
            "standard tier should out-earn the weak tier over {hands} hands, \
             got standard {standard_net:+} vs weak {weak_net:+}"
        );
    }

    /// EPIC-50 acceptance: the strong tier beats the standard tier with the
    /// real `decision:` knobs live. Both sides are drawn from their bundle
    /// pools: strong = `strengthen` base **plus** `decision.equity = fast{500}`
    /// + `ranges: position_aware`; standard = `ranges: position_aware` only.
    /// Adaptation is off on both (an orthogonal EPIC-47 toggle).
    ///
    /// Measured provenance:
    /// - **EPIC-50 (2026-07-17): +67,450 chips/100** over 12,000 hands with the
    ///   real equity engine on the strong seats — nearly 3× the pre-knob
    ///   `strengthen`-only edge (≈+24k), so the multi-way equity engine is now
    ///   the dominant strong lever; `strengthen`'s range/discipline tuning is
    ///   the profile layer beneath it. Isolating `strengthen`'s marginal
    ///   contribution now that equity is on is a follow-up measurement.
    /// - Adaptation is NOT a strong lever: adaptive-wrapped vs bare standard
    ///   measured a mild drag (−2.7k / −3.8k chips/100) — its value is modeling
    ///   *human* tendencies, invisible to a bot-vs-bot bench (EPIC-49 §1).
    ///
    /// Run via `make bench-tiers` (release; **~5 min — the strong seats run a
    /// 500-sample MC per postflop decision**, so hand count was cut 96k → 12k;
    /// the equity edge is large enough that the ordering holds easily at this
    /// volume). Entropy-dealt (see `run_matchup` note), not in the fast suite.
    #[test]
    #[ignore = "statistical bench (entropy-dealt); run via `make bench-tiers`"]
    fn strong_tier_beats_standard_tier() {
        let strong = builtin_strong_pool();
        let standard = builtin_standard_pool();
        let mut seats = Vec::new();
        for name in CORE_ARCHETYPES {
            seats.push(bare(pool_profile(&strong, name), true)); // strong tier
            seats.push(bare(pool_profile(&standard, name), false)); // standard tier
        }
        // Reduced from 96_000: the strong tier now runs the real equity engine
        // (500-sample MC per postflop decision), so this bench is compute-bound.
        // The equity edge is large, so the ordering holds with margin at this
        // volume; see the measured numbers in the EPIC-50 corrigendum.
        let hands = 12_000;
        let (strong_net, standard_net) = run_matchup(seats, hands);
        eprintln!(
            "strong {strong_net:+} vs standard {standard_net:+} over {hands} hands \
             ({:+.0} chips/100 differential)",
            (strong_net - standard_net) as f64 * 100.0 / hands as f64
        );
        assert!(
            strong_net > standard_net,
            "strong tier should out-earn the standard tier over {hands} hands, \
             got strong {strong_net:+} vs standard {standard_net:+}"
        );
    }
}

#[cfg(test)]
mod session_report_tests {
    use super::*;

    /// EPIC-49 Phase 3c: the chips/100 report nets every originally seated
    /// player against their starting stack. Chips are conserved, so the report
    /// must sum to zero; with no completed hands the rate is 0 rather than a
    /// division by zero.
    #[test]
    fn session_report_is_zero_sum_and_rate_scaled() {
        let seats_vec = vec![
            Seat::new(Player::new_with_chips("A".to_string(), 10_000)),
            Seat::new(Player::new_with_chips("B".to_string(), 10_000)),
        ];
        record_session_start(&seats_vec);
        let table = Table::nlh_from_seats(Seats::new(seats_vec), ForcedBets::new(50, 100));
        let mut session = PokerSession::new(table);
        session.start_hand().expect("start hand");

        // No completed hands yet: rates are 0, not NaN/inf.
        let report = session_report(&session.table, 0);
        assert_eq!(report.len(), 2);
        assert!(report.iter().all(|r| r.chips_per_100 == 0.0));

        // Play one hand to completion with minimal legal actions.
        while let Some(seat) = session.next_actor() {
            let to_call = session.table.to_call(seat);
            let chips = session
                .table
                .seats
                .get_seat(seat)
                .map_or(0, |s| s.player.chips);
            let action = if to_call == 0 {
                PlayerAction::Check
            } else if chips >= to_call {
                PlayerAction::Call
            } else {
                PlayerAction::AllIn
            };
            session.apply_action(seat, action).expect("legal action");
        }
        session.end_hand().expect("end hand");

        let report = session_report(&session.table, 1);
        assert_eq!(report.len(), 2, "every original seat stays in the report");
        let net_sum: i64 = report.iter().map(|r| r.net_chips).sum();
        assert_eq!(net_sum, 0, "chips are conserved, so the report is zero-sum");
        for r in &report {
            assert_eq!(r.hands_played, 1);
            #[allow(clippy::cast_precision_loss)]
            let expected_rate = r.net_chips as f64 * 100.0;
            assert!((r.chips_per_100 - expected_rate).abs() < f64::EPSILON);
        }
    }
}

#[cfg(test)]
mod position_awareness_tests {
    use super::*;
    use pkcore::bot::table_snapshot::SeatInfo;
    use pkcore::casino::position::Position;
    use pkcore::games::GamePhase;
    use pkcore::games::betting_structure::{BetTier, BettingStructure};

    /// Archetypes made (or re-graded to be) position-aware by
    /// `attach_archetype_playbook` (EPIC-49 Phase 2a): the five pkcore ships
    /// flat, plus the two whose pkcore playbooks lacked positional grades at
    /// the table sizes this app deals.
    const GRADED_ARCHETYPES: [&str; 7] = [
        "tight_aggressive",
        "loose_passive",
        "maniac",
        "abc",
        "short_stack_ninja",
        "tight_passive",
        "loose_aggressive",
    ];

    /// EPIC-49 Phase 2a: every non-joker profile in the pool carries a
    /// playbook with 6-max and 9-max entries, and the positional betting
    /// genuinely differentiates BTN from early position at both table sizes.
    #[test]
    fn every_archetype_is_position_aware() {
        for profile in builtin_standard_pool() {
            if profile.name == "joker" {
                // JokerDecider ignores its own profile; it morphs into a
                // (playbook-bearing) standard profile each hand.
                assert!(profile.playbook.is_none());
                continue;
            }
            let pb = profile
                .playbook
                .as_ref()
                .unwrap_or_else(|| panic!("{} should carry a playbook", profile.name));
            for seats in [6u8, 9u8] {
                assert!(
                    pb.for_seats(seats).is_some(),
                    "{} playbook missing a {seats}-max entry",
                    profile.name
                );
            }
            // Data-level divergence: the button plays more aggressively than
            // the earliest position at both table sizes.
            let btn9 = profile.betting_for(9, Position::BTN).aggression_factor;
            let utg9 = profile.betting_for(9, Position::UTG).aggression_factor;
            assert!(
                btn9 > utg9,
                "{}: 9-max BTN aggression ({btn9:?}) should exceed UTG ({utg9:?})",
                profile.name
            );
            let btn6 = profile.betting_for(6, Position::BTN).aggression_factor;
            let lj6 = profile.betting_for(6, Position::LJ).aggression_factor;
            assert!(
                btn6 > lj6,
                "{}: 6-max BTN aggression ({btn6:?}) should exceed LJ ({lj6:?})",
                profile.name
            );
        }
    }

    /// EPIC-49 Phase 2b: every profile carries a non-empty `three_bet` range,
    /// and every profile except `short_stack_ninja` a non-empty
    /// `call_three_bet` (the ninja's empty call range is intentional upstream
    /// — push-or-fold never flat-calls a 3-bet; pkcore has a test locking it).
    /// The decider does not consult these yet (upstream pkcore EPIC-36 wires
    /// them); carrying the data means the lineup lights up the moment it does.
    #[test]
    fn every_profile_carries_three_bet_ranges() {
        for profile in builtin_standard_pool() {
            assert!(
                !profile.range_strategy.three_bet.trim().is_empty(),
                "{} has an empty three_bet range",
                profile.name
            );
            if profile.name != "short_stack_ninja" {
                assert!(
                    !profile.range_strategy.call_three_bet.trim().is_empty(),
                    "{} has an empty call_three_bet range",
                    profile.name
                );
            }
        }
    }

    /// A fully authored 9-seat flop decision point — deal-independent (no
    /// entropy shuffle), so the RNG seed is the only source of variation.
    /// `logical_seat`/`dealer_button` are logical (button-relative) indices:
    /// with the button at 0, logical seat 0 is BTN and logical seat 3 is UTG
    /// (`Position::from_seat`, 9-max). `hole`/`to_call` shape which decider
    /// branch runs (value-bet, bluff, or facing-a-bet).
    fn nine_seat_snapshot(
        logical_seat: u8,
        hole: &str,
        to_call: usize,
        current_bet: usize,
    ) -> TableSnapshot<'static> {
        let stacks: Vec<SeatInfo> = (0..9u8)
            .map(|i| SeatInfo {
                id: uuid::Uuid::from_u128(u128::from(i) + 1),
                seat: i,
                name: format!("seat{i}"),
                chips: 9_800,
                bet: if i == 4 { current_bet } else { 0 },
                is_active: true,
            })
            .collect();
        TableSnapshot {
            seat: logical_seat,
            phase: GamePhase::Flop,
            board: "Ks 7h 2c".parse().expect("valid board"),
            hole_cards: hole.parse().expect("valid hole cards"),
            pot: 300,
            to_call,
            current_bet,
            min_raise: 100,
            my_chips: 9_800,
            stacks,
            big_blind: 100,
            betting_structure: BettingStructure::NoLimit,
            bet_tier: BetTier::Small,
            checked_this_street: false,
            dealer_button: Some(0),
            seat_count: 9,
            logical_seat: Some(logical_seat),
            opponent_stats: None,
        }
    }

    /// EPIC-49 Phase 2c: for each newly position-aware archetype, the decider's
    /// action stream as BTN differs from its stream as UTG on otherwise
    /// identical state. Two authored spots cover the graded knobs: a weak hand
    /// with no bet to face (bluff-frequency window) and a strong hand facing a
    /// bet (raise-probability window — the path that moves abc, whose bluff
    /// frequency is 0 everywhere). Seeded sweep ⇒ deterministic, never flaky.
    #[test]
    fn btn_and_utg_decisions_diverge_for_each_archetype() {
        let pool = builtin_standard_pool();
        let rule = RuleBasedDecider;

        for name in GRADED_ARCHETYPES {
            let profile = pool
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} missing from pool"));

            // Position::from_seat(9-max, button at logical 0): offset 0 = BTN,
            // offset 3 = UTG.
            let spots = [
                ("weak hand, unopened pot", "4d 3s", 0usize, 0usize),
                ("strong hand facing a bet", "Ad Kd", 100, 100),
            ];

            let mut diverged = false;
            for (label, hole, to_call, current_bet) in spots {
                let snap_btn = nine_seat_snapshot(0, hole, to_call, current_bet);
                let snap_utg = nine_seat_snapshot(3, hole, to_call, current_bet);
                assert_eq!(snap_btn.position(), Some(Position::BTN), "{label}");
                assert_eq!(snap_utg.position(), Some(Position::UTG), "{label}");

                let actions = |snap: &TableSnapshot| -> Vec<PlayerAction> {
                    (0u64..512)
                        .map(|seed| {
                            rule.decide_seeded(
                                profile,
                                snap,
                                &mut SmallRng::seed_from_u64(seed),
                            )
                        })
                        .collect()
                };
                if actions(&snap_btn) != actions(&snap_utg) {
                    diverged = true;
                    break;
                }
            }

            assert!(
                diverged,
                "{name}: BTN and UTG produced identical action streams in every \
                 authored spot — position awareness is not reaching the decider"
            );
        }
    }
}

// ── EPIC-50 Phase 3: browser-equity adoption ──────────────────────────────────

#[cfg(test)]
mod equity_adoption_tests {
    use super::*;
    use pkcore::bot::table_snapshot::SeatInfo;
    use pkcore::games::betting_structure::{BetTier, BettingStructure};

    /// A multi-way flop spot: hero (seat 0) plus `villains` still-active
    /// opponents, so the real multi-way equity engine sees a full field and
    /// prices the hand well below the proxy's opponent-blind absolute-rank
    /// estimate. `to_call`/`pot` set the pot-odds the decision compares against.
    fn multiway_snapshot(hole: &str, board: &str, to_call: usize, pot: usize, villains: u8) -> TableSnapshot<'static> {
        let n = villains + 1;
        let stacks: Vec<SeatInfo> = (0..n)
            .map(|i| SeatInfo {
                id: uuid::Uuid::from_u128(u128::from(i) + 1),
                seat: i,
                name: format!("seat{i}"),
                chips: 9_800,
                bet: if i == 1 { to_call } else { 0 },
                is_active: true,
            })
            .collect();
        TableSnapshot {
            seat: 0,
            phase: GamePhase::Flop,
            board: board.parse().expect("valid board"),
            hole_cards: hole.parse().expect("valid hole cards"),
            pot,
            to_call,
            current_bet: to_call,
            min_raise: 100,
            my_chips: 9_800,
            stacks,
            big_blind: 100,
            betting_structure: BettingStructure::NoLimit,
            bet_tier: BetTier::Small,
            checked_this_street: false,
            dealer_button: Some(0),
            seat_count: n,
            logical_seat: Some(0),
            opponent_stats: None,
        }
    }

    /// EPIC-50 Phase 3a: the strong tier's `equity` knob makes a demonstrably
    /// better decision than the proxy. Hero holds the nut flush draw with two
    /// overcards (A♠K♠ on Q♠7♠2♥): a monster *drawing* hand with ~15 outs, but
    /// no made hand yet — so the opponent-blind hand-rank proxy scores it as
    /// ace-high junk and folds to a bet, while the real multi-way engine prices
    /// its draw equity above the pot odds and continues. Deterministic —
    /// authored snapshot, seeded decider RNG, no deal.
    #[test]
    fn equity_knob_continues_a_strong_draw_the_proxy_misfolds() {
        // abc never bluffs (bluff_frequency 0), so a continue is equity-driven,
        // not a bluff — and a fold is a genuine fold.
        let proxy = BotProfile::abc();
        let equity = with_decision(BotProfile::abc(), strong_decision());

        // to_call 100 into pot 300 => pot odds 0.25. The proxy's ace-high sits
        // far below that; the real 3-way equity of the nut flush draw + two
        // overcards sits well above it.
        let snap = multiway_snapshot("As Ks", "Qs 7s 2h", 100, 300, 2);
        let rule = RuleBasedDecider;

        let fold_rate = |profile: &BotProfile| -> usize {
            (0u64..64)
                .filter(|&seed| {
                    matches!(
                        rule.decide_seeded(profile, &snap, &mut SmallRng::seed_from_u64(seed)),
                        PlayerAction::Fold
                    )
                })
                .count()
        };

        let proxy_folds = fold_rate(&proxy);
        let equity_folds = fold_rate(&equity);

        assert!(
            proxy_folds > equity_folds + 24,
            "the proxy should misfold the strong draw far more than the equity knob: \
             proxy {proxy_folds}/64 vs equity {equity_folds}/64"
        );
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;
    use serde_json::Value;

    fn state() -> Value {
        serde_json::from_str(&get_state()).expect("get_state returns valid JSON")
    }

    /// Run the same bot loop the JS front-end runs: step until it's the hero's
    /// turn or the hand is over.
    fn advance_past_bots() {
        while PHASE.with(|p| *p.borrow()) == SessionPhase::BotsActing {
            step_bot();
        }
    }

    #[test]
    fn undo_round_trips_a_human_action() {
        init_game(0.42);
        advance_past_bots();

        let before = state();
        // If everyone folded to a blind before the hero acted there's nothing to
        // exercise; a re-deal is out of scope for this unit test.
        if before["phase"] != "WaitingForHuman" {
            return;
        }
        assert_eq!(
            before["can_undo"],
            Value::Bool(false),
            "no human action yet this hand, so nothing is undoable"
        );

        // Take the simplest legal action (never a sized bet, so no amount math).
        let legal: Vec<String> = before["legal_actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let action = ["Check", "Call", "Fold"]
            .into_iter()
            .find(|a| legal.iter().any(|l| l == a))
            .expect("a preflop hero always has at least Fold");

        let after_act: Value = serde_json::from_str(&human_action(&format!(
            r#"{{"action":"{action}","amount":0}}"#
        )))
        .unwrap();
        assert_eq!(
            after_act["can_undo"],
            Value::Bool(true),
            "the action applied, so it must be undoable"
        );

        let after_undo: Value = serde_json::from_str(&undo_action()).unwrap();
        assert_eq!(after_undo["phase"], "WaitingForHuman");
        assert_eq!(after_undo["can_undo"], Value::Bool(false));
        for field in ["hand_number", "pot", "to_call", "board", "hero"] {
            assert_eq!(
                after_undo[field], before[field],
                "field `{field}` should match the pre-action state after undo"
            );
        }
    }

    #[test]
    fn undo_with_empty_history_is_a_noop() {
        init_game(0.99);
        let before = state();
        // Undo with nothing on the stack must not panic or error — it returns
        // the current state unchanged.
        let after: Value = serde_json::from_str(&undo_action()).unwrap();
        assert_eq!(after["can_undo"], Value::Bool(false));
        assert_eq!(after["hand_number"], before["hand_number"]);
        assert!(after.get("error").is_none(), "no-op undo must not set an error");
    }
}
