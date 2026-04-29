//! WASM bindings for pkarena0 — single human player vs 8 bots in NLHE.
//!
//! Game state is held in three `thread_local!` singletons so the JS side can
//! call simple functions without passing state back and forth.

use std::cell::RefCell;

use pkcore::bot::profile::BotProfile;
use pkcore::casino::action::PlayerAction;
use pkcore::casino::game::ForcedBets;
use pkcore::casino::session::PokerSession;
use pkcore::casino::state::PlayerState;
use pkcore::casino::table::event::TableAction;
use pkcore::analysis::name::HandRankName;
use pkcore::casino::table::winnings::Winnings;
use pkcore::casino::table_no_cell::{PlayerNoCell, SeatNoCell, SeatsNoCell, TableNoCell};
use pkcore::card::Card;
use pkcore::cards::Cards;
use pkcore::games::GamePhase;
use pkcore::hand_history::{
    Action as HhAction, ActionType, HandCollection, HandHistory, Outcome,
};
use pkcore::suit::Suit;
use std::str::FromStr;
use rand::SeedableRng;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
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

thread_local! {
    static SESSION: RefCell<Option<PokerSession>> = const { RefCell::new(None) };
    static BOTS: RefCell<Vec<BotProfile>> = const { RefCell::new(Vec::new()) };
    static RNG: RefCell<SmallRng> = RefCell::new(SmallRng::seed_from_u64(0));
    static PHASE: RefCell<SessionPhase> = const { RefCell::new(SessionPhase::Uninitialized) };
    /// Chip counts at the start of the current hand (before blinds), indexed by seat.
    static HAND_START_CHIPS: RefCell<Vec<(u8, usize)>> = const { RefCell::new(Vec::new()) };
    /// Accumulated hand histories for the session; exported via get_session_yaml().
    static COLLECTION: RefCell<HandCollection> = RefCell::new(HandCollection::new());
    /// One-shot error message surfaced to the UI without locking the game.
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    /// One-shot hand result populated by next_hand(), consumed by build_game_state().
    static LAST_HAND_RESULT: RefCell<Option<Vec<PotResult>>> = const { RefCell::new(None) };
    /// When true, seat 0 is a bot (Arena mode); step_bot() never sets WaitingForHuman.
    static IS_ALL_BOT: RefCell<bool> = const { RefCell::new(false) };
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
    let bots: Vec<BotProfile> = profile_pool.into_iter().take(8).collect();
    let bot_names: Vec<String> = bots.iter().map(|b| b.name.clone()).collect();

    let mut seats_vec = vec![SeatNoCell::new(PlayerNoCell::new_with_chips(
        "You".to_string(),
        10_000,
    ))];
    for name in &bot_names {
        seats_vec.push(SeatNoCell::new(PlayerNoCell::new_with_chips(
            name.clone(),
            10_000,
        )));
    }

    // Capture chip counts BEFORE start_hand() posts blinds.
    let start_chips: Vec<(u8, usize)> = seats_vec
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u8, s.player.chips))
        .collect();
    HAND_START_CHIPS.with(|h| *h.borrow_mut() = start_chips);
    COLLECTION.with(|c| *c.borrow_mut() = HandCollection::new());

    let table = TableNoCell::nlh_from_seats(
        SeatsNoCell::new(seats_vec),
        ForcedBets::new(50, 100),
    );

    let mut session = PokerSession::new(table);
    if session.start_hand().is_err() {
        return error_state("Failed to deal first hand");
    }

    BOTS.with(|b| *b.borrow_mut() = bots);
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
    let bots: Vec<BotProfile> = profile_pool.into_iter().take(9).collect();
    let bot_names: Vec<String> = bots.iter().map(|b| b.name.clone()).collect();

    let seats_vec: Vec<SeatNoCell> = bot_names
        .iter()
        .map(|name| SeatNoCell::new(PlayerNoCell::new_with_chips(name.clone(), 10_000)))
        .collect();

    let start_chips: Vec<(u8, usize)> = seats_vec
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u8, s.player.chips))
        .collect();
    HAND_START_CHIPS.with(|h| *h.borrow_mut() = start_chips);
    COLLECTION.with(|c| *c.borrow_mut() = HandCollection::new());

    let table = TableNoCell::nlh_from_seats(
        SeatsNoCell::new(seats_vec),
        ForcedBets::new(50, 100),
    );

    let mut session = PokerSession::new(table);
    if session.start_hand().is_err() {
        return error_state("Failed to deal first hand");
    }

    BOTS.with(|b| *b.borrow_mut() = bots);
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
            session.set_blinds(ForcedBets::new(
                small_blind as usize,
                big_blind as usize,
            ));
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
        player_snapshot: Vec<(u8, String, usize, Option<String>)>,
        shuffled_deck_str: Option<String>,
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
                    let hole_str = table
                        .dealt_hole_cards
                        .get(&seat_num)
                        .and_then(|bc| {
                            let s: String = bc
                                .as_slice()
                                .iter()
                                .filter(|c| **c != Card::BLANK)
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join(" ");
                            if s.is_empty() { None } else { Some(s) }
                        });
                    Some((seat_num, seat.player.handle.clone(), starting, hole_str))
                })
                .collect();

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
                    let hh = HandHistory::from_table_state(
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
                    COLLECTION.with(|c| c.borrow_mut().push(hh));

                    // Build per-pot winner summary for the UI.
                    let pot_results: Vec<PotResult> = winnings.vec().iter().map(|pot_win| {
                        let seats: Vec<u8> = (0u8..9)
                            .filter(|&i| pot_win.equity.seats.contains(i))
                            .collect();
                        let names: Vec<String> = seats.iter().map(|&seat| {
                            s.player_snapshot
                                .iter()
                                .find(|(sn, _, _, _)| *sn == seat)
                                .map(|(_, name, _, _)| name.clone())
                                .unwrap_or_default()
                        }).collect();
                        PotResult {
                            seats,
                            names,
                            amount: pot_win.equity.chips,
                            hand: hand_rank_name_to_str(pot_win.eval.hand_rank.name),
                        }
                    }).collect();
                    LAST_HAND_RESULT.with(|r| *r.borrow_mut() = Some(pot_results));
                }
            }
        });
    }

    // pkcore's TableNoCell::reset() does not clear event_log, so it accumulates
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
        }
    });

    let funded = SESSION.with(|s| {
        s.borrow()
            .as_ref()
            .map_or(0, |sess| sess.count_funded())
    });

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

    let Some(results) = hh.results.as_deref() else { return desc; };
    let winner = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Win | Outcome::Tie))
        .max_by(|a, b| {
            a.pot_won
                .unwrap_or(0.0)
                .partial_cmp(&b.pot_won.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    let Some(w) = winner else { return desc; };

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
    let Some(rank) = chars.next() else { return String::new(); };
    let suit_char = chars.next().unwrap_or(' ');
    let rank_str = if rank == 'T' { "10".to_string() } else { rank.to_string() };
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
    let Some(streets) = &hh.streets else { return steps; };
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
    let Some(streets) = &hh.streets else { return events; };

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

    let max_seat = hh.players.iter().map(|p| p.seat as usize).max().unwrap_or(0);
    let table_size = max_seat.max(button as usize) + 1;
    let mut seats_vec: Vec<SeatNoCell> = (0..table_size)
        .map(|_| SeatNoCell::new(PlayerNoCell::default()))
        .collect();
    for p in &hh.players {
        seats_vec[p.seat as usize] =
            SeatNoCell::new(PlayerNoCell::new_with_chips(p.name.clone(), p.stack as usize));
    }
    let seats = SeatsNoCell::new(seats_vec);
    let mut table = TableNoCell::nlh_from_seats(seats, ForcedBets::new(sb, bb));
    table.button = button;

    table.act_forced_bets().map_err(|e| e.to_string())?;

    let hole_entries: Vec<(u8, String)> = hh
        .players
        .iter()
        .filter_map(|p| p.hole_cards.as_ref().map(|h| (p.seat, h.clone())))
        .collect();
    let hole_refs: Vec<(u8, &str)> = hole_entries
        .iter()
        .map(|(s, h)| (*s, h.as_str()))
        .collect();
    table.inject_hole_cards(&hole_refs).map_err(|e| e.to_string())?;

    let events = build_event_list(hh);
    let total_steps = events.len() + 1;
    let target = target_step.min(events.len());

    let mut last_label = "Hand begins".to_string();
    let mut current_seat: Option<u8> = None;

    for event in events.iter().take(target) {
        match event {
            ReplayEvent::Action { seat, action, label } => {
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
    let pot_committed: usize = table
        .seats
        .0
        .iter()
        .map(|s| s.player.bet)
        .sum::<usize>()
        + table.pot;
    let street = street_from_board(table.board.len(), SessionPhase::BotsActing);

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
            let cards: Vec<String> = s
                .cards
                .as_slice()
                .iter()
                .filter(|c| **c != Card::BLANK)
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

    let next = SESSION.with(|s| {
        s.borrow_mut().as_mut().and_then(|sess| sess.next_actor())
    });

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
            let (action, call_amount, allin_chips, name, hole_cards) = SESSION.with(|s| {
                BOTS.with(|b| {
                    RNG.with(|r| {
                        let bots = b.borrow();
                        let mut rng = r.borrow_mut();
                        // In all-bot mode seat N → bot N directly.
                        // In normal mode seats 1-8 → bots 0-7 (seat 0 is human).
                        let bot_idx = if all_bot { seat as usize } else { (seat as usize).saturating_sub(1) };
                        let session_ref = s.borrow();
                        if let Some(session) = session_ref.as_ref() {
                            let call_amt = session.table.to_call(seat);
                            let chips = session.table.seats.get_seat(seat)
                                .map_or(0, |s| s.player.chips);
                            let name = session.table.seats.get_seat(seat)
                                .map(|s| s.player.handle.clone())
                                .unwrap_or_default();
                            let hole_cards: Vec<String> = session.table.seats.get_seat(seat)
                                .map_or_else(Vec::new, |s| {
                                    s.cards.as_slice().iter()
                                        .filter(|c| **c != Card::BLANK)
                                        .map(card_to_str)
                                        .collect()
                                });
                            if let Some(bot) = bots.get(bot_idx) {
                                let act = bot.decide(&session.table, seat, &mut *rng);
                                return (act, call_amt, chips, name, hole_cards);
                            }
                        }
                        (PlayerAction::Fold, 0, 0, String::new(), Vec::new())
                    })
                })
            });

            let action_label = match &action {
                PlayerAction::Fold => "folds".to_string(),
                PlayerAction::Check => "checks".to_string(),
                PlayerAction::Call => format!("calls ${}", call_amount),
                PlayerAction::Bet(n) => format!("bets ${}", n),
                PlayerAction::Raise(n) => format!("raises to ${}", n),
                PlayerAction::AllIn => format!("goes all-in ${}", allin_chips),
            };

            let err = SESSION.with(|s| {
                s.borrow_mut().as_mut()
                    .and_then(|sess| sess.apply_action(seat, action).err())
            });
            if err.is_some() {
                let _ = SESSION.with(|s| {
                    s.borrow_mut().as_mut()
                        .and_then(|sess| sess.apply_action(seat, PlayerAction::Fold).err())
                });
            }

            serde_json::json!({
                "done": false,
                "seat": seat,
                "name": name,
                "action_label": action_label,
                "hole_cards": hole_cards,
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

        let street = street_from_board(table.board.len(), phase_val);
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
        let hero_chips = table
            .seats
            .get_seat(0)
            .map_or(0, |s| s.player.chips);
        let max_bet = hero_chips;

        let legal_actions = derive_legal_actions(to_call, hero_chips, table.bet);

        // Hero view — always show hole cards.
        let hero_view = seat_to_player_view(
            table, 0, dealer_seat, sb_seat, bb_seat, true,
        );

        // Bot views — reveal hole cards at HandComplete/Showdown for in-hand bots.
        let reveal_bot_cards = phase_val == SessionPhase::HandComplete
            && table.board.len() == 5;

        let players: Vec<PlayerView> = (1..table.seats.0.len())
            .map(|i| {
                let seat = i as u8;
                let in_hand = table
                    .seats
                    .get_seat(seat)
                    .map_or(false, |s| is_in_hand(&s.player.state));
                seat_to_player_view(
                    table,
                    seat,
                    dealer_seat,
                    sb_seat,
                    bb_seat,
                    reveal_bot_cards && in_hand,
                )
            })
            .collect();

        // Consume any one-shot values so they surface to the UI exactly once.
        let last_error = LAST_ERROR.with(|e| e.borrow_mut().take());
        let last_result = LAST_HAND_RESULT.with(|r| r.borrow_mut().take());

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
        };

        serde_json::to_string(&state)
            .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string())
    })
}

fn seat_to_player_view(
    table: &TableNoCell,
    seat: u8,
    dealer_seat: u8,
    sb_seat: u8,
    bb_seat: u8,
    show_cards: bool,
) -> PlayerView {
    let Some(s) = table.seats.get_seat(seat) else {
        return empty_player_view(seat);
    };

    let hole_cards: Option<Vec<String>> = if show_cards {
        let cards: Vec<String> = s
            .cards
            .as_slice()
            .iter()
            .filter(|c| **c != Card::BLANK)
            .map(card_to_str)
            .collect();
        if cards.is_empty() { None } else { Some(cards) }
    } else {
        // For bots when not at showdown, indicate cards are face-down (2 blanks).
        let dealt = s.cards.as_slice().iter().filter(|c| **c != Card::BLANK).count();
        if dealt > 0 && is_in_hand(&s.player.state) {
            Some(vec!["__".to_string(); dealt])
        } else {
            None
        }
    };

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

fn street_from_board(board_len: usize, phase: SessionPhase) -> String {
    match board_len {
        0 => "Preflop",
        3 => "Flop",
        4 => "Turn",
        5 => {
            if phase == SessionPhase::HandComplete {
                "Showdown"
            } else {
                "River"
            }
        }
        _ => "Showdown",
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
        HandRankName::StraightFlush  => Some("Straight Flush".to_string()),
        HandRankName::FourOfAKind    => Some("Four of a Kind".to_string()),
        HandRankName::FullHouse      => Some("Full House".to_string()),
        HandRankName::Flush          => Some("Flush".to_string()),
        HandRankName::Straight       => Some("Straight".to_string()),
        HandRankName::ThreeOfAKind   => Some("Three of a Kind".to_string()),
        HandRankName::TwoPair        => Some("Two Pair".to_string()),
        HandRankName::Pair           => Some("Pair".to_string()),
        HandRankName::HighCard       => Some("High Card".to_string()),
        HandRankName::Invalid        => None,
    }
}

