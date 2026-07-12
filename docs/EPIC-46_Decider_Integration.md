# EPIC-46: Full BotDecider Integration

> **One-line:** Replace the single hardcoded `BotProfile::decide()` call with a
> per-seat `BotDecider` architecture in the web loop — unlocking per-hand hooks
> (fixing the inert Joker), decider wrappers (`ExploitativeDecider`), seeded
> determinism, and every follow-on bot EPIC.

## Status

| Component | Status |
|---|---|
| Per-seat `Vec<(BotProfile, Box<dyn BotDecider>)>` replacing the profile pool | Planned |
| `on_new_hand_with_rng` called at each `start_hand` | Planned |
| `JokerDecider` actually constructed for the joker seat | Planned |
| Web builds `TableSnapshot` itself and calls `decider.decide_seeded(...)` | Planned |
| Rejected-action fallback surfaced (log + counter) instead of silent `Fold` | Planned |
| Playwright regression: bot play unchanged for non-joker seats | Planned |

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
actions (a pkcore decider bug or a web-side state mismatch) as quiet folds. It
should stay as a safety net but become *observable*.

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

### Phase 1 — Decider-per-seat
- [ ] 1a. Introduce `BotSeat { profile, decider }`; build pools with
  `RuleBasedDecider` for the eight archetypes, `JokerDecider` for joker
  (play mode `src/lib.rs:93-96`, arena mode `146-149`).
- [ ] 1b. Refactor `step_bot` to snapshot + `decide_seeded`
  (`src/lib.rs:1076-1088`).
- [ ] 1c. Fire `on_new_hand_with_rng` for every bot at hand start.

### Phase 2 — Observability & regression safety
- [ ] 2a. Surface the forced-Fold fallback (hand log + console + counter).
- [ ] 2b. Rust unit test: same seed ⇒ identical action sequence pre/post
  refactor for a non-joker lineup.
- [ ] 2c. Playwright spec: arena mode multi-hand run completes; forced-Fold
  counter is 0. (Use Turbo speed per `docs/known-issues.md` conventions.)

---

## Key Files

| File | Role |
|---|---|
| `src/lib.rs:93-96,146-149` | bot pool construction (play/arena) |
| `src/lib.rs:1044-1126` | `step_bot` — snapshot + decide_seeded |
| `src/lib.rs:1109-1114` | forced-Fold fallback → observable |
| pkcore `src/bot/decider.rs:69,144,330` | `BotDecider` trait, `RuleBasedDecider`, `JokerDecider` |
| pkcore `src/bot/table_snapshot.rs:193` | `TableSnapshot::from_table` |

---

## Verification

```bash
make build                                  # wasm-pack build must stay green
cargo test                                  # seeded-equivalence unit test
npx playwright test                         # arena regression spec
```

Acceptance: (1) non-joker bots produce identical seeded action sequences
before/after; (2) the joker demonstrably plays different styles across hands
(assert differing aggression profile over N seeded hands); (3) forced-Fold
counter is 0 across a 20-hand arena run.

---

## Open Questions

- Should the joker's per-hand style be surfaced in the UI (e.g. a subtle
  indicator), or stay hidden as a gameplay surprise?
- Keep `BotProfile::decide` for the human-hint path (if ever added), or
  standardize on the trait everywhere?
