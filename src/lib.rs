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

/// Initialise a new session with 9 players (seat 0 = human, seats 1-8 = bots).
///
/// Seeds the RNG from `rand_seed`, deals the first hand, and advances bots
/// until it is the human's turn. Returns a `GameState` JSON string.
#[wasm_bindgen]
pub fn init_game(rand_seed: f64) -> String {
    IS_ALL_BOT.with(|f| *f.borrow_mut() = false);
    // Seed RNG.
    RNG.with(|r| *r.borrow_mut() = SmallRng::seed_from_u64(rand_seed.to_bits()));

    // Build 9-player table: hero at seat 0, bots at seats 1-8.
    // Shuffle all available profiles and pick 8 so the lineup varies each game.
    let mut profile_pool = BotProfile::default_profiles();
    profile_pool.push(BotProfile::joker());
    RNG.with(|r| profile_pool.shuffle(&mut *r.borrow_mut()));
    let adaptive = ADAPTIVE.with(|a| *a.borrow());
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
    COLLECTION.with(|c| *c.borrow_mut() = HandCollection::new());
    REGISTRY.with(|r| *r.borrow_mut() = StatsRegistry::new());
    FORCED_FOLD_COUNT.with(|c| *c.borrow_mut() = 0);

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

    // Pick 9 bot profiles so every seat has a bot (seat 0 included).
    let mut profile_pool = BotProfile::default_profiles();
    profile_pool.push(BotProfile::joker());
    RNG.with(|r| profile_pool.shuffle(&mut *r.borrow_mut()));
    let adaptive = ADAPTIVE.with(|a| *a.borrow());
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
    COLLECTION.with(|c| *c.borrow_mut() = HandCollection::new());
    REGISTRY.with(|r| *r.borrow_mut() = StatsRegistry::new());
    FORCED_FOLD_COUNT.with(|c| *c.borrow_mut() = 0);

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

    let apply_result = SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().as_mut() {
            session.apply_action(0, action).err().map(|e| e.to_string())
        } else {
            Some("No active session".to_string())
        }
    });

    if let Some(err) = apply_result {
        // Store the error so build_game_state() can surface it, but keep the
        // phase as WaitingForHuman so the action buttons remain usable.
        LAST_ERROR.with(|e| *e.borrow_mut() = Some(err));
        return build_game_state();
    }

    PHASE.with(|p| *p.borrow_mut() = SessionPhase::BotsActing);
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
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_result: Option<Vec<PotResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    showdown: Option<Vec<ShowdownPlayer>>,
    forced_fold_count: u32,
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
                error: None,
                last_result: None,
                showdown: None,
                forced_fold_count: 0,
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
            error: last_error,
            last_result,
            showdown,
            forced_fold_count,
        };

        serde_json::to_string(&state)
            .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string())
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
