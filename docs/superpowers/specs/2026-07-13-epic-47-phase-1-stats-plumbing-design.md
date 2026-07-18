# EPIC-47 Phase 1 — Stats Plumbing (design)

> **Date:** 2026-07-13
> **Scope:** Phase 1 of [EPIC-47](../../EPIC-47_Adaptive_Bots_Player_Stats.md) only.
> **Depends on:** EPIC-46 (decider-per-seat architecture — landed, verified).

## One-line

Record every completed hand *with player identities* and feed it into a
session-scoped `StatsRegistry`, so opponent stats accumulate. Nothing reads
those stats yet — bot decisions stay byte-for-byte identical. This is the
load-bearing substrate for Phases 2–4 (injection, adaptation, HUD).

## Goal & non-goals

**Goal:** After Phase 1, `StatsRegistry` holds per-player `PlayerStats`
(VPIP/PFR/AF/…) keyed by a stable `Uuid`, refreshed each hand, reset each
session. Zero observable gameplay change.

**Non-goals (deferred to later phases / out of scope):**

- Reading stats in `step_bot` (`from_table_with_stats`) — Phase 2.
- Wrapping deciders in `ExploitativeDecider` — Phase 3.
- HUD badges — Phase 4.
- `player-stats-persistence` (uses `std::fs`, will not link on wasm).
- Cross-session persistence, adaptivity toggle, default-on policy — these are
  EPIC-47 Open Questions and do not touch Phase 1.

## Verified assumptions (pkcore 0.2.1)

All checked against `~/.cargo/registry/.../pkcore-0.2.1/src` before writing:

- `player-stats = []` is a **pure feature flag** — no extra dependencies; it
  un-gates `analysis::player_stats`. `player-stats-persistence` is the flag
  that pulls in `std::fs` (not added).
- `StatsRegistry`: `new()`, `get(id: Uuid) -> Option<&PlayerStats>`,
  `ingest_hand(&HandHistory)` (`player_stats.rs:269,275,306`).
- `PlayerStats::vpip() -> Option<f64>`, `confidence() -> Confidence`
  (`player_stats.rs:119,201`).
- `PlayerSnapshot` type alias = `(u8, String, usize, Option<String>,
  Option<Uuid>)`, publicly exported (`hand_history.rs:194`).
- `HandHistory::from_table_state_with_ids(...)` takes `&[PlayerSnapshot]` and is
  otherwise arg-identical to `from_table_state`, which merely lifts the 4-tuple
  with `None` ids (`hand_history.rs:242,311`).
- `Player.id: Uuid` (`player.rs:29`), minted once via `Uuid::new_v4()` in
  `Player::new_with_chips`.

## Design

### 1. Feature flag

`Cargo.toml`: add `"player-stats"` to the pkcore feature list.

```toml
pkcore = { version = "0.2.1", features = [
    "bot-profiles", "hand-histories", "player-stats" ] }
```

### 2. Identity threading

The single change that makes the registry non-inert. Without a `Uuid` per
seat, the registry has nothing to correlate on.

- Widen the `PreEnd.player_snapshot` field type from the 4-tuple
  `Vec<(u8, String, usize, Option<String>)>` to pkcore's exported
  `pkcore::hand_history::PlayerSnapshot` (the 5-tuple).
- At snapshot construction (`src/lib.rs` ~312), append `Some(seat.player.id)`
  to each seat's tuple.
- Swap the `HandHistory::from_table_state(...)` call (`src/lib.rs` ~420) for
  `HandHistory::from_table_state_with_ids(...)` — same arguments, 5-tuple
  `player_snapshot`.

**Uuid stability (verified):** `Player` instances are created once in
`init_game` / `init_bot_game` and stored in the session's `Seats`. `next_hand`
reuses the same session/table — it never re-mints players — so `Player.id` is
stable across hands within a session. (Empty seats use `Player::default()`'s
nil `Uuid`, but empty seats are never ingested.)

### 3. Registry lifecycle

```rust
thread_local! {
    static REGISTRY: RefCell<StatsRegistry> = RefCell::new(StatsRegistry::new());
}
```

- **Ingest** — immediately **before** `COLLECTION.with(|c| c.borrow_mut().push(hh))`
  (`src/lib.rs` ~433): `REGISTRY.with(|r| r.borrow_mut().ingest_hand(&hh))`.
  Order matters: `HandCollection::push(&mut self, hand: HandHistory)` takes `hh`
  **by value** (moves it, `hand_history.rs:988`), so ingest — which borrows
  `&hh` — must run first, then push consumes `hh`.
- **Reset** — alongside `COLLECTION = HandCollection::new()` in both
  `init_game` (~126) and `init_bot_game`, reset the registry so stats are
  session-scoped, matching the collection's lifecycle.

### 4. Test (acceptance 1d)

A Rust unit test drives N hands (following the existing multi-hand test pattern
around `src/lib.rs:1706`) and asserts **plumbing invariants**, not statistical
values.

**Rationale — why not assert VPIP magnitudes:** pkcore's `start_hand` shuffles
the deck with an *entropy* RNG, not our seeded `SmallRng` (documented in the
EPIC-46 parity test: independently dealt games cannot be compared). Asserting a
specific VPIP, or even a reliable maniac-vs-nit ordering, would flake on small
samples. The test therefore asserts what Phase 1 actually delivers — correct
identity correlation and ingest:

- After N hands, `registry.get(bot_id)` returns `Some` for each seated bot's id.
- Each bot id is present and **distinct** (no id collision / no nil ids).
- Returned `vpip()` is either `None` or within `[0.0, 1.0]`; the sample count /
  `confidence()` reflects that N hands were ingested.

## Files touched

| File | Change |
|---|---|
| `Cargo.toml` | add `player-stats` feature |
| `src/lib.rs` (~305, PreEnd) | `player_snapshot` field → 5-tuple `PlayerSnapshot` |
| `src/lib.rs` (~312) | append `Some(seat.player.id)` |
| `src/lib.rs` (~420) | `from_table_state` → `from_table_state_with_ids` |
| `src/lib.rs` (new thread_local) | `REGISTRY` |
| `src/lib.rs` (~126, init_game / init_bot_game) | reset `REGISTRY` |
| `src/lib.rs` (~433, next_hand) | `ingest_hand(&hh)` |
| `src/lib.rs` (tests) | registry plumbing-invariant test |

## Verification

```bash
cargo check --target wasm32-unknown-unknown   # feature builds on wasm
cargo test                                     # registry plumbing test + existing suite
make build                                     # wasm bundle stays green
```

**Acceptance:** (1) stats accumulate with correct, distinct identity
attribution per bot; (2) zero behavior change — the existing EPIC-46
decider-parity and repair-ladder tests still pass unchanged; (3) wasm bundle
still builds.
