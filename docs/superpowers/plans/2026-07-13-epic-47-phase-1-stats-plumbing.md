# EPIC-47 Phase 1 — Stats Plumbing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record every completed hand *with player identities* and feed it into a session-scoped `StatsRegistry`, so opponent stats accumulate — with zero observable gameplay change.

**Architecture:** Enable pkcore's `player-stats` feature; widen the hand-history snapshot from a 4-tuple to pkcore's 5-tuple `PlayerSnapshot` (appending each seat's stable `Player` `Uuid`); switch the history builder to `from_table_state_with_ids`; add a `REGISTRY` `thread_local!` that ingests one `HandHistory` per completed hand and resets per session. Nothing reads the registry yet.

**Tech Stack:** Rust, `wasm-bindgen`/wasm32 target, pkcore 0.2.1, `cargo test` (native), `wasm-pack` (`make build`).

## Global Constraints

- **pkcore version:** `0.2.1` (exact — do not bump).
- **Feature added:** `player-stats` only. Do **NOT** add `player-stats-persistence` — it uses `std::fs` and will not link on wasm32.
- **Zero behavior change:** the existing EPIC-46 tests (`decider_path_parity_tests`, `repair_ladder_tests`, `street_tests`, `sort_tests`) must pass **unchanged**.
- **Must build on wasm:** `cargo check --target wasm32-unknown-unknown` and `make build` must stay green.
- **Uuid stability (verified):** `Player` instances are minted once in `init_game`/`init_bot_game`; `next_hand` reuses them, so `Player.id` is stable across hands within a session. Do not re-mint players anywhere.

---

### Task 1: Accumulate opponent stats by player identity

This is a single cohesive task: identity threading and the registry are one deliverable — identity threading with no registry does nothing observable, and a registry with no threaded ids correlates nothing. They share one test cycle.

**Files:**
- Modify: `Cargo.toml` (pkcore feature list)
- Modify: `src/lib.rs:24` (hand_history import — add `PlayerSnapshot`)
- Modify: `src/lib.rs` (new `use` for `StatsRegistry`)
- Modify: `src/lib.rs:52-76` (`thread_local!` block — add `REGISTRY`)
- Modify: `src/lib.rs:126` and `src/lib.rs:175` (session reset — reset `REGISTRY`)
- Modify: `src/lib.rs:278` (`PreEnd.player_snapshot` field type)
- Modify: `src/lib.rs:312` (snapshot construction — append `Some(seat.player.id)`)
- Modify: `src/lib.rs:420` (`from_table_state` → `from_table_state_with_ids`)
- Modify: `src/lib.rs:433` (ingest before push)
- Test: `src/lib.rs` (new `#[cfg(test)] mod stats_plumbing_tests`)

**Interfaces:**
- Consumes (pkcore 0.2.1, verified):
  - `pkcore::hand_history::PlayerSnapshot = (u8, String, usize, Option<String>, Option<Uuid>)`
  - `HandHistory::from_table_state_with_ids(hand_num: usize, ts_secs: u64, button: u8, forced: &ForcedBets, player_snapshot: &[PlayerSnapshot], board_str: &str, winnings: &Winnings, event_log: &[TableAction], ending_stacks: &[(u8, usize)], source: &str, shuffled_deck: Option<String>) -> HandHistory`
  - `pkcore::analysis::player_stats::StatsRegistry::new() -> StatsRegistry`
  - `StatsRegistry::ingest_hand(&mut self, hand: &HandHistory)`
  - `StatsRegistry::get(&self, id: Uuid) -> Option<&PlayerStats>`
  - `PlayerStats::vpip(&self) -> Option<f64>`
  - `Player.id: Uuid` (field), `Uuid::is_nil(&self) -> bool`
- Produces: a session-scoped `REGISTRY` `thread_local!` populated one hand at a time; no public API surface (later phases read it).

---

- [ ] **Step 1: Write the failing test**

Add this new module at the end of `src/lib.rs` (after the closing `}` of the last existing `#[cfg(test)] mod ...`). It drives real hands with real `Player` `Uuid`s and asserts *plumbing invariants* (ids present, distinct, non-nil, stable; `vpip` in range) — not statistical magnitudes, because `start_hand` shuffles from the entropy RNG (same deal-independence rationale the EPIC-46 parity test documents).

```rust
#[cfg(test)]
mod stats_plumbing_tests {
    use super::*;
    use pkcore::analysis::player_stats::StatsRegistry;
    use pkcore::hand_history::{HandHistory, PlayerSnapshot};

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
                    let action = hero_action(&session);
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test stats_registry_correlates_players_by_identity 2>&1 | tail -20`
Expected: **compile error** — `unresolved import pkcore::analysis::player_stats` (the `player-stats` feature is off) and `no field/variant PlayerSnapshot`. This is the expected red state; the feature and imports don't exist yet.

- [ ] **Step 3: Enable the `player-stats` feature**

In `Cargo.toml`, change the pkcore dependency line:

```toml
pkcore = { version = "0.2.1", features = ["bot-profiles", "hand-histories", "player-stats"] }
```

(from `features = ["bot-profiles", "hand-histories"]`).

- [ ] **Step 4: Add imports**

In `src/lib.rs:24`, add `PlayerSnapshot` to the existing hand_history import:

```rust
use pkcore::hand_history::{
    Action as HhAction, ActionType, HandCollection, HandHistory, Outcome, PlayerSnapshot,
};
```

Immediately below the existing `use pkcore::analysis::...` imports (near line 8-9), add:

```rust
use pkcore::analysis::player_stats::StatsRegistry;
```

- [ ] **Step 5: Add the `REGISTRY` thread_local**

In the `thread_local!` block, immediately after the `COLLECTION` declaration (`src/lib.rs:60`), add:

```rust
    /// Session-scoped opponent stats (EPIC-47 Phase 1). Fed one `HandHistory`
    /// per completed hand, keyed by `Player` `Uuid`. Reset per session
    /// alongside `COLLECTION`. Populated but not yet read — later phases
    /// (injection/adaptation/HUD) consume it. `StatsRegistry::new()` is not
    /// `const`, so this mirrors `COLLECTION`'s non-const initializer.
    static REGISTRY: RefCell<StatsRegistry> = RefCell::new(StatsRegistry::new());
```

- [ ] **Step 6: Reset `REGISTRY` per session**

In `init_game`, immediately after the `COLLECTION` reset at `src/lib.rs:126`
(`COLLECTION.with(|c| *c.borrow_mut() = HandCollection::new());`), add:

```rust
    REGISTRY.with(|r| *r.borrow_mut() = StatsRegistry::new());
```

Do the identical addition in `init_bot_game`, immediately after its `COLLECTION` reset at `src/lib.rs:175`.

- [ ] **Step 7: Thread player ids into the snapshot**

Widen the `PreEnd.player_snapshot` field type at `src/lib.rs:278`:

```rust
        player_snapshot: Vec<PlayerSnapshot>,
```

(from `Vec<(u8, String, usize, Option<String>)>`).

At the snapshot-construction closure return at `src/lib.rs:312`, append the id:

```rust
                    Some((seat_num, seat.player.handle.clone(), starting, hole_str, Some(seat.player.id)))
```

(from `Some((seat_num, seat.player.handle.clone(), starting, hole_str))`).

- [ ] **Step 8: Switch to the id-aware history builder and ingest before push**

At `src/lib.rs:420`, change the call from `HandHistory::from_table_state(` to `HandHistory::from_table_state_with_ids(` — all arguments are unchanged; `&s.player_snapshot` is now the 5-tuple `&[PlayerSnapshot]` the function expects.

At `src/lib.rs:433`, add the ingest line **before** the existing push (order matters — `HandCollection::push` takes `hh` by value and moves it):

```rust
                    REGISTRY.with(|r| r.borrow_mut().ingest_hand(&hh));
                    COLLECTION.with(|c| c.borrow_mut().push(hh));
```

- [ ] **Step 9: Run the new test to verify it passes**

Run: `cargo test stats_registry_correlates_players_by_identity 2>&1 | tail -15`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 10: Run the full suite to confirm zero behavior change**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass, including the unchanged EPIC-46 tests (`convenience_and_decider_paths_agree_at_every_decision`, `joker_morphs_style_across_hands`, `repair_ladder_tests::*`, `street_tests::*`, `sort_tests::*`). Total should be the prior 8 + 1 new = **9 passed**.

- [ ] **Step 11: Verify the wasm build stays green**

Run: `cargo check --target wasm32-unknown-unknown 2>&1 | tail -5 && make build 2>&1 | tail -5`
Expected: `cargo check` finishes with no errors; `make build` ends with `✨   Done` and `Your wasm pkg is ready`.

- [ ] **Step 12: Commit**

```bash
git add Cargo.toml src/lib.rs docs/superpowers/plans/2026-07-13-epic-47-phase-1-stats-plumbing.md
git commit -m "feat(EPIC-47): Phase 1 — accumulate opponent stats by player identity

Enable pkcore player-stats feature; thread stable Player Uuids through the
hand-history snapshot (5-tuple + from_table_state_with_ids); add a
session-scoped REGISTRY thread_local that ingests one HandHistory per
completed hand and resets per session. No decision logic reads it yet, so
gameplay is unchanged (EPIC-46 tests pass verbatim)."
```

---

## Notes / Known Limitations

- **What the test guards:** the pkcore contract (identity threads through `from_table_state_with_ids`, the registry correlates by `Uuid`) exercised with *real* `Player` ids driven through *real* hands, plus id stability/non-nil-ness. It replicates production's exact call sequence rather than invoking `next_hand()` directly, because the `#[wasm_bindgen]` entry points call `web_sys` and are not cleanly runnable under native `cargo test` (the EPIC-46 tests follow the same manual-drive convention). The production call site itself is covered by later phases, where stats become observable via the HUD and Playwright specs.
- **No `uuid` import needed:** the test only reads `Player.id` and calls `.is_nil()`/`!=` on the values, so it needs no `use uuid::Uuid`.
