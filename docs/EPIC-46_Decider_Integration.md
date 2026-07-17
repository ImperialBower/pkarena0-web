# EPIC-46: Full BotDecider Integration

> **One-line:** Replace the single hardcoded `BotProfile::decide()` call with a
> per-seat `BotDecider` architecture in the web loop — unlocking per-hand hooks
> (fixing the inert Joker), decider wrappers (`ExploitativeDecider`), seeded
> determinism, and every follow-on bot EPIC.

## Status

| Component | Status |
|---|---|
| Per-seat `BotSeat { profile, decider }` replacing the profile pool | **Complete** (`src/lib.rs:52`; pool `BOTS`, `src/lib.rs:70`) |
| `on_new_hand_with_rng` called at each `start_hand` | **Complete** (`notify_bots_new_hand`, `src/lib.rs:1727,1732`) |
| `JokerDecider` actually constructed for the joker seat | **Complete** (`make_bot_seat`, `src/lib.rs:1710,1713`) |
| Web builds `TableSnapshot` itself and calls `decider.decide_seeded(...)` | **Complete** (`src/lib.rs:1323,1332` — `from_table_with_stats` since EPIC-47) |
| Rejected-action fallback surfaced (log + counter) instead of silent `Fold` | **Complete** (`FORCED_FOLD_COUNT`, `src/lib.rs:103,1368`) |
| Rejected bet/raise *repaired* (clamp→call→all-in) instead of folded | **Complete** — pulled forward from follow-up (`apply_bot_action`, `src/lib.rs:1778`) |
| Rust unit tests: decider-path parity, joker morph, repair ladder | **Complete** (`decider_path_parity_tests`, `src/lib.rs:2432`; `repair_ladder_tests`, `src/lib.rs:2584`) |
| Playwright regression: arena multi-hand run, forced-fold counter bounded | **Complete** — 5/5 arena specs pass (`tests/arena.spec.ts`, incl. `:99`) |

**Closed 2026-07-16** (branch `EPIC-46`, at `c6e855f`) — all phases shipped; see
the [corrigendum](#implementation-corrigendum-2026-07-16-branch-epic-46).

**EPIC number:** allocated from the shared ImperialBower sequence
(pkdealer holds EPIC-44/45; 46 was the next free number).

---

## Context

A capability audit (2026-07-12) found that pkarena0-web contains **no bot
decision logic of its own** — every bot action flows through exactly one call:

```rust
let act = bot.decide(&session.table, seat, &mut *rng);   // src/lib.rs:1087
```

`BotProfile::decide` (pkcore `src/bot/profile.rs:947`) is a convenience method
that **hardcodes `RuleBasedDecider`** (`profile.rs:956`). Consequences:

- **The Joker is a fake.** `BotProfile::joker()` is added to the pool
  (`src/lib.rs:94,147`) but `JokerDecider` (pkcore `src/bot/decider.rs:330`) —
  the component that re-rolls a random style each hand via the `on_new_hand`
  hook — is never constructed. The joker seat plays as a static GTO clone; its
  own range/betting fields are documented as "never used in practice"
  (`profile.rs:449-451`).
- **No wrapper composition.** `ExploitativeDecider::wrap(inner)`
  (pkcore `src/bot/exploitative_decider.rs:61`) can wrap any `BotDecider` to
  add opponent adaptation, but there is no decider object to wrap.
- **No stats injection.** `TableSnapshot::from_table_with_stats`
  (pkcore `src/bot/table_snapshot.rs:325`) exists, but the convenience method
  builds its own snapshot without stats.
- **No per-hand lifecycle.** `BotDecider::on_new_hand` /
  `on_new_hand_with_rng` (`decider.rs:74`) are never called.

The `BotDecider` trait (pkcore `src/bot/decider.rs:69`) is object-safe and
`Send + Sync`; pkdealer's `pkdealer_agent_rules` (EPIC-23) already consumes it
this way. pkarena0-web is the only downstream consumer still on the
convenience path.

### Current web invocation path

```
step_bot()  [src/lib.rs:1044-1126, driven by JS ~1s ticks]
  → session.next_actor()                    [src/lib.rs:1052]
  → bot.decide(&table, seat, rng)           [src/lib.rs:1087]  ← the choke point
  → session.apply_action(seat, action)      [src/lib.rs:1107]
  → on error: silently force Fold           [src/lib.rs:1109-1114]
```

The silent-Fold fallback deserves attention on its own: it masks illegal bot
actions as quiet folds. Those illegal actions fall into **three** categories,
not two:

1. a pkcore decider bug,
2. a web-side state mismatch, and
3. **a legal-*intent* action with an illegal *amount*** — most commonly a
   `Raise` that `RuleBasedDecider` sized below the NLHE minimum raise
   increment, which the engine rejects with `InsufficientIncrement`.

Category 3 was **routine, not exceptional**: measured at ~2 forced folds per
20-hand arena run (range 0–6; see `docs/known-issues.md`) *before* this EPIC.

Folding is the worst substitution for a rejected raise. Rather than defer it,
this EPIC pulled the fix forward: `apply_bot_action` (`src/lib.rs:1778`) now
walks a repair ladder — clamp an under-sized `Bet`/`Raise` up to
`min_raise_to()`, else call/check, else all-in — and only folds as a last
resort. A repaired action does **not** increment `FORCED_FOLD_COUNT`.

The counter's meaning therefore changed: it no longer measures category-3
sizing rejections (those are now repaired), only genuinely **unrepairable**
errors — a pkcore decider bug or a web-side state mismatch. Post-ladder it
should trend toward zero; the arena spec keeps a loose upper bound as a
gross-breakage regression guard rather than asserting an exact rate.

---

## Goals

- Give every bot seat an explicit `(BotProfile, Box<dyn BotDecider>)` pair.
- Fire `on_new_hand_with_rng` at hand start so `JokerDecider` works and future
  deciders get their lifecycle hook.
- Build `TableSnapshot` in the web loop and call
  `decide_seeded(&profile, &snapshot, rng)` — the seam where EPIC-47 injects
  `opponent_stats` and EPIC-48/49 inject configured profiles.
- Keep behavior byte-identical for the eight non-joker archetypes.
- Make the rejected-action fallback visible (hand-log entry + console warn).

## Scope

**In scope:** `src/lib.rs` bot-pool and `step_bot` refactor; joker wiring;
lifecycle hooks; fallback observability; tests.

**Out of scope:** enabling new pkcore features (`player-stats`, `equity` —
EPIC-47/48); changing any decision *logic*; UI changes beyond a hand-log line.

---

## Design

### Bot pool

Today (`src/lib.rs:93-96` play mode, `146-149` arena mode) the pool is
`Vec<BotProfile>`. It becomes:

```rust
struct BotSeat {
    profile: BotProfile,
    decider: Box<dyn BotDecider>,
}
// joker seat: Box::new(JokerDecider::default())
// everyone else: Box::new(RuleBasedDecider)
```

`thread_local!` storage follows the existing `BOTS` pattern. `Box<dyn
BotDecider>` is fine in `wasm32`'s single thread; the trait's `Send + Sync`
bound costs nothing here.

### step_bot

```rust
// replaces src/lib.rs:1076-1088
let snapshot = TableSnapshot::from_table(&session.table, seat);
let act = bot.decider.decide_seeded(&bot.profile, &snapshot, &mut *rng);
```

EPIC-47 later swaps `from_table` for `from_table_with_stats`. Note
`decide_seeded` keeps the existing `SmallRng` (`src/lib.rs:50,89,143`) so
seeded runs stay reproducible.

### Hand lifecycle

In `next_hand()` / `start_hand` paths, after the new hand begins:

```rust
for bot in bots.iter() {
    bot.decider.on_new_hand_with_rng(&mut *rng);
}
```

This is the single change that makes the joker actually morph per hand.

### Fallback observability

`src/lib.rs:1109-1114` keeps forcing `Fold` on `apply_action` error, but:
- pushes a marker into the UI hand log ("engine rejected {action}, folded"),
- `web_sys::console::warn_1` with seat, action, and error,
- increments a session counter surfaced in `get_state()` for tests to assert
  it stays zero.

---

## Work Items

### Phase 1 — Decider-per-seat ✅
- [x] 1a. Introduce `BotSeat { profile, decider }` (`src/lib.rs:52`); build
  pools with `RuleBasedDecider` for the eight archetypes, `JokerDecider` for
  joker (`make_bot_seat`, `src/lib.rs:1710,1713`).
- [x] 1b. Refactor `step_bot` (`src/lib.rs:1257`) to snapshot + `decide_seeded`
  (`src/lib.rs:1323,1332`).
- [x] 1c. Fire `on_new_hand_with_rng` for every bot at hand start
  (`notify_bots_new_hand`, `src/lib.rs:1727,1732`).

### Phase 2 — Observability & regression safety ✅
- [x] 2a. Surface the forced-Fold fallback (hand log + console + counter);
  repair rejected bet/raise sizings instead of folding them
  (`apply_bot_action`, `src/lib.rs:1778`; counter `src/lib.rs:1368`).
- [x] 2b. Rust unit tests: per-decision parity for a non-joker seat
  (`convenience_and_decider_paths_agree_at_every_decision`, `src/lib.rs:2469`),
  joker style-morph over N hands (`joker_morphs_style_across_hands`,
  `src/lib.rs:2558`), and repair-ladder clamp/passthrough
  (`undersized_raise_is_clamped_up_to_the_minimum`, `src/lib.rs:2609`;
  `legal_action_applies_unchanged`, `src/lib.rs:2634`).
- [x] 2c. Playwright spec: arena mode multi-hand run completes; forced-Fold
  counter stays below a loose bound (not exactly 0 — see acceptance #3).
  (Use Turbo speed per `docs/known-issues.md` conventions.) Verified:
  `npx playwright test arena.spec.ts` → 5/5 pass (`:99` in 22.4s).

---

## Key Files

Line references are as of close (`c6e855f`, 2026-07-16); the `## Context` and
`## Design` sections above deliberately keep their *audit-time* (2026-07-12,
pre-EPIC) coordinates — see corrigendum §4.

| File | Role |
|---|---|
| `src/lib.rs:52,70` | `BotSeat` type + `BOTS` pool `thread_local!` |
| `src/lib.rs:1710` | `make_bot_seat` — per-seat decider construction |
| `src/lib.rs:1257,1323,1332` | `step_bot` — snapshot + `decide_seeded` |
| `src/lib.rs:1727` | `notify_bots_new_hand` — `on_new_hand_with_rng` fan-out |
| `src/lib.rs:1778` | `apply_bot_action` — repair ladder |
| `src/lib.rs:103,1368` | `FORCED_FOLD_COUNT` — unrepairable-rejection counter |
| pkcore `src/bot/decider.rs:69,144,330` | `BotDecider` trait, `RuleBasedDecider`, `JokerDecider` |
| pkcore `src/bot/table_snapshot.rs:193` | `TableSnapshot::from_table` |

---

## Verification

```bash
make build                                  # wasm-pack build must stay green
cargo test                                  # seeded-equivalence unit test
npx playwright test                         # arena regression spec
```

Acceptance: (1) the non-joker decision path is behaviourally identical
before/after — i.e. `BotProfile::decide(&table, seat, rng)` and
`RuleBasedDecider::decide_seeded(&profile, &TableSnapshot::from_table(..), rng)`
agree at every decision (asserted per-decision on identical state, since the
full-game sequence *cannot* be byte-identical: the joker now draws from the
shared RNG each hand, and `start_hand`'s deck shuffle uses the entropy RNG, not
our seed); (2) the joker demonstrably plays different styles across hands
(assert ≥2 distinct aggression profiles over N seeded hands); (3) the
forced-Fold counter stays **below a loose bound** across a 20-hand arena run.
Note the bound's rationale changed once the repair ladder landed: the old
~2/run baseline came from `InsufficientIncrement` raises being force-folded,
but those are now *repaired* (clamped) and no longer counted, so the expected
count trends toward zero. The bound (`tests/arena.spec.ts` uses `< 20`) is a
gross-breakage guard against a genuinely broken decider or state mismatch, not
a rate to tune — asserting exactly `0` would be a fair stretch goal now, but a
loose bound avoids flaking on any residual unrepairable edge case
(see `docs/known-issues.md`).

---

## Implementation corrigendum (2026-07-16, branch `EPIC-46`)

Both phases shipped. Verified at close: `cargo test` → **20 passed, 0 failed,
5 ignored** (the 5 are EPIC-49's `#[ignore]`d fixture generators and
entropy-dealt benches, not skipped EPIC-46 work); `npx playwright test
arena.spec.ts` → 5/5.

1. **The repair ladder was pulled into scope, not deferred.** The EPIC opened
   scoping category-3 rejections (a `Raise` sized below the NLHE minimum
   increment) as *observable* — log, warn, count — with the fix left to a
   follow-up. Folding is the worst possible substitution for a rejected raise,
   and the category was routine rather than exceptional (~2 forced folds per
   20-hand run), so `apply_bot_action` (`src/lib.rs:1778`) landed here instead
   (`6c6fbf0`): clamp an under-sized `Bet`/`Raise` up to `min_raise_to()`, else
   call/check, else all-in, and only then fold.
2. **So `FORCED_FOLD_COUNT`'s meaning changed, and acceptance #3 with it.** The
   counter no longer measures sizing rejections — those are repaired and
   deliberately *not* counted — only genuinely unrepairable ones (a pkcore
   decider bug or a web-side state mismatch), which should trend to zero. The
   original acceptance target was calibrated against the ~2/run baseline; the
   shipped gate is a loose `< 20` (`tests/arena.spec.ts`) as a gross-breakage
   guard. Asserting exactly `0` is now a fair stretch goal, held back only to
   avoid flaking on a residual edge case.
3. **Byte-identical full-game parity was unimplementable; per-decision parity
   replaced it.** Goal 4 ("keep behavior byte-identical for the eight non-joker
   archetypes") cannot be tested end-to-end: the joker now draws from the shared
   `RNG` (`src/lib.rs:71`) each hand, and pkcore's `start_hand` shuffles from the
   entropy thread-local, so there is no seeded deck to replay. The gate is
   instead per-decision agreement on identical state
   (`convenience_and_decider_paths_agree_at_every_decision`, `src/lib.rs:2469`).
   EPIC-49 independently re-hit this same no-seeded-deck constraint (its
   corrigendum §3) and resolved it statistically.
4. **Citation coordinates are split by design.** `## Context` and `## Design`
   keep their audit-time (2026-07-12) line numbers — including
   `bot.decide(&table, seat, rng)` at `src/lib.rs:1087`, the choke point this
   EPIC *deleted*. Those cite code that no longer exists and repointing them
   would corrupt the record. `## Status`, `## Work Items`, and `## Key Files`
   were refreshed to close-time (`c6e855f`); EPIC-47/48/49 grew `src/lib.rs`
   past 3,800 lines and displaced every original target by 300–1,300 lines.
5. **The `make_bot_seat` seam took an extra parameter.** The design sketched
   per-seat construction as a function of the profile alone; EPIC-47 needed
   adaptivity at construction time, so the shipped signature is
   `make_bot_seat(profile, adaptive)` (`src/lib.rs:1710`). Likewise the
   `on_new_hand_with_rng` loop lives in its own `notify_bots_new_hand()`
   (`src/lib.rs:1727`) rather than inline at the `start_hand` call sites.
6. **Inherited debt / handoffs:** both Open Questions below remain open —
   neither blocked the close. The `decide_seeded` seam (`src/lib.rs:1332`)
   delivered on its purpose: EPIC-47 injected `opponent_stats` through it and
   EPIC-49 injected configured profiles, neither needing to reopen this EPIC.

| Phase | Status at close |
|---|---|
| Phase 1 — decider-per-seat | **Complete** |
| Phase 2 — observability & regression safety | **Complete** (repair ladder pulled forward into scope) |

---

## Open Questions

*(Both still open at close — see corrigendum §6.)*

- Should the joker's per-hand style be surfaced in the UI (e.g. a subtle
  indicator), or stay hidden as a gameplay surprise?
- Keep `BotProfile::decide` for the human-hint path (if ever added), or
  standardize on the trait everywhere?
