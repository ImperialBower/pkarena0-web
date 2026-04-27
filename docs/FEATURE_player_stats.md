# Player Stats (HUD)

**Status:** Planned / not yet shipped. This doc captures the design and motivation for leveraging pkcore's EPIC-26 Player Stats layer (introduced in pkcore 0.0.51) inside pkarena0-web.

**pkcore version:** 0.0.51 (bumped from 0.0.50 on branch `tracker`) — drop-in; `cargo check --target wasm32-unknown-unknown` passes with no source edits.

## Context

pkcore 0.0.51 ships **EPIC-26 — Player Stats**: a full opponent-modeling layer (VPIP, PFR, 3-bet%, c-bet%, etc.) plus persistence. The release also includes smaller behavioral refinements (`Outcome::Fold` distinction, `Position::from_seat` no-panic, new `HandCollection` filters, `TableSnapshot::checked_this_street`).

The release-audit doc in pkcore (`docs/RELEASE_AUDIT_0.0.51.md`) prescribed source edits to `src/lib.rs:257` and `:295` for the `from_table_state` 5-tuple change. **Those edits are no longer required** — the post-audit "softening" commit (`dbe3e6e` in pkcore) reverted `from_table_state` to its 4-tuple shape and parked the new identity-threading capability behind a sibling `from_table_state_with_ids`. The 4-tuple internally lifts each tuple to the 5-tuple form with `None` as the `Option<Uuid>`.

This doc catalogs what's in 0.0.51 that pkarena0-web could leverage, ranked by value-to-effort.

---

## Opportunity 1 — Per-opponent HUD stats (HIGH value)

The headline 0.0.51 feature. Gated behind a new `player-stats` Cargo feature flag (not currently enabled in `Cargo.toml`).

### What you get

`pkcore::analysis::player_stats::PlayerStats` exposes:

| Method | Stat |
|---|---|
| `vpip()` | Voluntarily put $ in pot |
| `pfr()` | Preflop raise % |
| `three_bet_pct()` / `four_bet_pct()` | 3-bet / 4-bet % |
| `fold_to_three_bet_pct()` | Fold to 3-bet % |
| `cbet_pct()` / `fold_to_cbet_pct()` | Continuation bet stats |
| `aggression_factor()` / `aggression_freq()` | Postflop aggression |
| `wtsd()` / `w_at_sd()` | Went to showdown / won at showdown |
| `confidence()` | `Confidence` enum (None / Low / Medium / High) tied to sample size |

The `Confidence` enum is the load-bearing nicety — it lets the UI hide stats until enough hands are sampled to be meaningful, instead of showing "100% VPIP" after one hand.

`StatsRegistry::ingest_collection(&HandCollection)` ingests the entire collection in one call; `ingest_hand(&HandHistory)` is incremental.

### Why pkarena0-web is well-positioned for this

You already maintain a `COLLECTION: HandCollection` thread_local at `src/lib.rs:48`, and you push every completed hand into it inside `next_hand()` at `src/lib.rs:368`. That's the exact feed source `StatsRegistry` wants.

### Implementation sketch

1. **Feature flag**: in `Cargo.toml:14`, add `"player-stats"` to the features list:
   ```toml
   pkcore = { version = "0.0.51", features = ["bot-profiles", "hand-histories", "player-stats"] }
   ```
   *Skip* `player-stats-persistence` — it writes YAML to disk via `std::fs`, which won't link in `wasm32-unknown-unknown`.

2. **Identity threading**: switch the `from_table_state` call at `src/lib.rs:368` to `from_table_state_with_ids`. Extend the `player_snapshot` field type at `src/lib.rs:257` from
   ```rust
   Vec<(u8, String, usize, Option<String>)>
   ```
   to
   ```rust
   Vec<(u8, String, usize, Option<String>, Option<uuid::Uuid>)>
   ```
   and append `Some(seat.player.id)` to each tuple constructed at `src/lib.rs:295`. Without this step, every `PlayerEntry.player_id` is `None`, the registry can't correlate hands to seats, and the whole feature is inert.

3. **Registry**: add a thread_local alongside `COLLECTION`:
   ```rust
   static REGISTRY: RefCell<StatsRegistry> = RefCell::new(StatsRegistry::new());
   ```
   After each `collection.push(hh.clone())`, call `registry.ingest_hand(&hh)`.

4. **Surface in `get_state()` JSON**: for each bot seat, look up `registry.get(bot_uuid)` and serialize the stats the UI wants to display (probably VPIP, PFR, aggression, plus the confidence tier).

5. **UI**: render small HUD badges next to each bot's name. Match the existing seat element ID conventions (`seat-{0-8}-{stat-name}`).

### Caveat — bots won't adapt

`RuleBasedDecider` deliberately ignores opponent stats (locked in by the regression test `rule_based_decider_ignores_opponent_stats` in pkcore — see commit `b3828d8`'s insight). So this is purely a *display* feature for the human player. Bots won't play differently against a tight opponent vs a maniac.

This is fine for a single-player app, but worth being clear about: the HUD is information for **you**, not for the bots.

---

## Opportunity 2 — Session review filters (MEDIUM value, no feature flag)

Three new `HandCollection` methods drop in unconditionally:

| Method | Purpose |
|---|---|
| `hands_by_player(uuid)` | Iterator over hands involving a specific player |
| `hands_by_position(Position::CO)` | Iterator filtered by position |
| `showdowns_only()` | Iterator over hands that reached showdown |

These would power a "session review" panel — natural pairing with the lifetime P&L feature you just shipped (see `FEATURE_pnl.md`):

> **Session so far:** 47 hands · 12 went to showdown · vs LoosePlayer1 you're 5W-3L · button hands +$420, BB hands -$180

Lower-effort than the HUD path (no feature flag, no signature changes), but lower-impact too — it's a stats screen, not a live overlay.

Note: `showdowns_only()` quality depends on `Outcome::Fold` being distinct from `Outcome::Lose` (see opportunity 3) — that's now correct in 0.0.51.

---

## Opportunity 3 — Free correctness wins (zero work)

These improve silently without any code change:

- **`Outcome::Fold` vs `Outcome::Lose`** — `from_table_state` now emits `Outcome::Fold` for folded seats instead of conflating them with `Outcome::Lose`. If you ever read your own histories back for analysis, fold-counting becomes a one-liner instead of an event-log walk. No current code path reads histories, so no immediate change — but it's there when needed.
- **`Position::from_seat` no-panic** — used to panic with arithmetic underflow when `button > seat + seat_count`; now returns `None`. Strictly safer in any bot path that constructs positions.

---

## Opportunity 4 — `TableSnapshot::checked_this_street` (LOW value)

New `bool` field on `TableSnapshot` populated by `from_table()`. Could power a "✓ Checked" UI badge on bot seats during the current street. Requires plumbing through `get_state()` JSON since `TableSnapshot` lives on the bot side, not the JS-facing state.

Probably not worth the lift unless paired with a broader UI refresh.

---

## Skip list

| What | Why skip |
|---|---|
| `player-stats-persistence` feature | Filesystem writes via `std::fs`, won't link on `wasm32-unknown-unknown`. Browser-side persistence belongs in `localStorage`, not pkcore. |
| `SimTable::with_stats_registry` | pkarena0-web doesn't use `SimTable` — it's a one-hand-at-a-time interactive loop, not a batch simulator. |
| Updating bots to use opponent stats | `RuleBasedDecider` ignores `opponent_stats` by design (regression test in pkcore locks this). Would require writing a custom `Decider` impl. Out of scope for a "single player vs bots" demo. |

---

## Recommended sequencing

1. **Now**: take the 0.0.50 → 0.0.51 bump as drop-in. Pocket the `Outcome::Fold` and `Position::from_seat` improvements.
2. **Next**: opportunity 1 (HUD stats). Highest visible payoff. Roughly: feature-flag flip + 5-tuple migration + registry wiring + JSON surface + UI badges. Estimate 1–2 sessions of work.
3. **Later**: opportunity 2 (session review panel). Pairs naturally with P&L; do it alongside any future stats-screen work.
4. **Skip for now**: opportunities 4 (checked-this-street) and the persistence layer.

## Stale memory cleanup

While auditing, two memory entries were found stale:

- `project_pkarena0.md` references `path = "../pkcore"` — actual `Cargo.toml` is version-pinned (`pkcore = "0.0.51"`).
- `project_pkcore_chip_audit_bug.md` references pkcore 0.0.43 / 0.0.44 — the bug was fixed seven releases ago. The `had_audit_failure` workaround in `next_hand()` remains as defense-in-depth.

Neither blocks any work in this backlog; flagged here for future memory-maintenance passes.
