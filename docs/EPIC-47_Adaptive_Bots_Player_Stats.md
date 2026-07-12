# EPIC-47: Adaptive Bots — Player Stats & Exploitative Play

> **One-line:** Enable pkcore's complete-but-dormant opponent-modeling stack
> (EPIC-26 `StatsRegistry` → EPIC-27 `ExploitativeDecider` → EPIC-28 trained
> `ExploitConfig`s) so the bots adapt to how the human actually plays — plus a
> HUD so the human can see the same stats.

## Status

| Component | Status |
|---|---|
| Enable `player-stats` feature in `Cargo.toml` | Planned |
| Identity threading: `from_table_state_with_ids` + 5-tuple snapshot | Planned |
| `StatsRegistry` thread_local; `ingest_hand` per completed hand | Planned |
| `TableSnapshot::from_table_with_stats` in `step_bot` | Planned |
| Wrap bot deciders in `ExploitativeDecider` (EPIC-27) | Planned |
| Ship an EPIC-28 trained `ExploitConfig` as embedded YAML | Planned |
| Per-seat HUD badges (VPIP/PFR/AF + `Confidence` gate) | Planned |
| Adaptivity toggle in setup UI (off = today's behavior) | Planned |

**Depends on:** [EPIC-46](EPIC-46_Decider_Integration.md) (decider-per-seat
architecture — required to wrap deciders and inject stats).
**Supersedes / absorbs:** `docs/FEATURE_player_stats.md` (0.0.51-era plan;
its HUD design carries forward as Phase 4 here, and its "bots won't adapt —
out of scope" caveat is exactly what this EPIC removes).

---

## Context

pkcore 0.2.1 ships the full adaptive stack, all marked **Complete** upstream:

- **EPIC-26** — `PlayerStats` (`src/analysis/player_stats.rs:55`): VPIP, PFR,
  3-bet%, fold-to-c-bet, AF, WTSD, … each `Option<f64>` plus a
  `Confidence` tier (`:201`) gating on sample size. `StatsRegistry` (`:256`)
  keyed by `Uuid`, fed by `ingest_hand(&HandHistory)` (`:306`).
- **EPIC-27** — `ExploitativeDecider<D>`
  (`src/bot/exploitative_decider.rs:44`): wraps any `BotDecider`; when the
  snapshot carries `opponent_stats`, applies `adjust_profile`
  (`src/bot/exploit.rs:197`) driven by `ExploitConfig` (`exploit.rs:36`, 8
  deviation rules with min-hands gates). No stats attached ⇒ exact no-op.
- **EPIC-28** — `ExploitTrainer` produced tuned `ExploitConfig`s,
  YAML-serializable behind the `bot-training` feature (training happens
  offline, native-side; the web app only *consumes* a trained YAML).

None of it reaches pkarena0-web today, for three independent reasons:

1. **Feature off.** `Cargo.toml` enables only `bot-profiles, hand-histories`.
   (Verified 2026-07-12: `cargo check --target wasm32-unknown-unknown
   --no-default-features --features bot-profiles,hand-histories,equity,player-stats`
   passes cleanly on pkcore 0.2.1.)
2. **No identities.** The hand-history snapshot is a 4-tuple
   (`src/lib.rs:307`) and `HandHistory::from_table_state` (`src/lib.rs:413`)
   leaves every `player_id` as `None` — a `StatsRegistry` would have nothing
   to correlate on. `Player.id: Uuid` exists
   (pkcore `src/casino/table/player.rs:29`) and
   `from_table_state_with_ids` (pkcore `src/hand_history.rs:207`) is the
   intended entry point.
3. **No injection seam.** Pre-EPIC-46, the web app calls
   `BotProfile::decide`, which builds its snapshot without stats and cannot
   be wrapped.

The feed already exists: `COLLECTION: HandCollection` (`src/lib.rs:55`)
receives every completed hand (`src/lib.rs:426`). This EPIC taps that feed.

### Why this matters for gameplay

Today a maniac and a nit get identical treatment from the bots. With this
EPIC, a human who folds to every c-bet gets barreled more; a calling station
gets value-bet thinner and bluffed less. This is the single biggest
"the bots feel alive" upgrade available at zero algorithmic cost — the
algorithms shipped upstream months ago.

---

## Goals

- Bots adapt to observed play (human *and* other bots — the registry is
  seat-agnostic; arena mode gets bot-vs-bot adaptation for free).
- Human sees a per-seat HUD once `Confidence` clears `Low`.
- Zero behavior change when the adaptivity toggle is off or sample sizes are
  below `ExploitConfig` min-hands gates.
- No persistence writes (wasm): registry is session-scoped. (localStorage
  persistence is an open question, not a goal.)

## Scope

**In scope:** feature flag, identity threading, registry wiring, decider
wrapping, one embedded trained `ExploitConfig`, HUD badges, toggle.

**Out of scope:** running `ExploitTrainer` in the browser;
`player-stats-persistence` (uses `std::fs` — will not link on wasm);
`SimTable` (batch simulator, not the interactive loop); new stats beyond
what `PlayerStats` derives.

---

## Design

### Identity threading (the load-bearing step)

Extend `player_snapshot` (`src/lib.rs:307`) from
`Vec<(u8, String, usize, Option<String>)>` to
`Vec<(u8, String, usize, Option<String>, Option<Uuid>)>` appending
`Some(seat.player.id)`, and switch `src/lib.rs:413` to
`from_table_state_with_ids`. Without this, every downstream piece is inert.

Bot `Uuid`s must be stable across hands within a session — verify the pool
construction (`src/lib.rs:93-96,146-149`) reuses `Player` instances rather
than reminting ids per hand.

### Registry

```rust
thread_local! {
    static REGISTRY: RefCell<StatsRegistry> = RefCell::new(StatsRegistry::new());
}
// next_hand(), right after COLLECTION.push (src/lib.rs:426):
REGISTRY.with(|r| r.borrow_mut().ingest_hand(&hh));
```

Reset alongside `COLLECTION` at session start (`src/lib.rs:117,163`).

### Injection + wrapping (builds on EPIC-46)

```rust
// step_bot, replacing the plain snapshot:
let snapshot = TableSnapshot::from_table_with_stats(&table, seat, Some(&registry));
let act = bot.decider.decide_seeded(&bot.profile, &snapshot, rng);
```

Bot construction wraps when adaptivity is on:

```rust
Box::new(ExploitativeDecider::wrap_with_config(RuleBasedDecider, trained_config))
// joker: ExploitativeDecider::wrap_with_config(JokerDecider::default(), ...)
```

`ExploitConfig` YAML ships as an embedded `include_str!` asset (same pattern
as other static data), parsed once at session start. Start with pkcore's
default config; swap in an EPIC-28 trained one after arena validation.

### HUD (Phase 4 — carried over from FEATURE_player_stats.md)

Per-seat badges (VPIP / PFR / AF), rendered only when
`stats.confidence() >= Confidence::Low`; serialize through `get_state()`
JSON; SVG element ids follow the existing `seat-{n}-…` convention.

---

## Work Items

### Phase 1 — Plumbing (no behavior change)
- [ ] 1a. Add `player-stats` to the pkcore features in `Cargo.toml`.
- [ ] 1b. Identity threading: 5-tuple snapshot + `from_table_state_with_ids`.
- [ ] 1c. `REGISTRY` thread_local; ingest per hand; reset per session.
- [ ] 1d. Test: after N seeded hands, `registry.get(bot_id)` returns stats
  with plausible VPIP for a loose vs tight archetype.

### Phase 2 — Stats reach the deciders
- [ ] 2a. `from_table_with_stats` in `step_bot` (EPIC-46 seam).
- [ ] 2b. Regression: unwrapped `RuleBasedDecider` ignores stats (pkcore locks
  this via `rule_based_decider_ignores_opponent_stats`) — seeded action
  sequences unchanged by Phase 1+2a alone.

### Phase 3 — Adaptation on
- [ ] 3a. Wrap deciders in `ExploitativeDecider::wrap_with_config` behind a
  setup-screen toggle (default: on for arena mode, off for play mode?
  see Open Questions).
- [ ] 3b. Embed a trained `ExploitConfig` YAML; fall back to
  `ExploitConfig::default()` on parse failure (console warn).
- [ ] 3c. Seeded arena test: with adaptation on, at least one decision differs
  after the min-hands gate clears; with it off, sequences are identical.

### Phase 4 — HUD
- [ ] 4a. Serialize per-seat `{vpip, pfr, af, confidence}` in `get_state()`.
- [ ] 4b. UI badges with `Confidence` gating.
- [ ] 4c. Playwright: badges absent at hand 1, present for active bots after
  ~20 Turbo-speed hands.

---

## Key Files

| File | Role |
|---|---|
| `Cargo.toml` | add `player-stats` feature |
| `src/lib.rs:307,413` | 5-tuple snapshot + `from_table_state_with_ids` |
| `src/lib.rs:55,117,163,426` | collection/registry lifecycle |
| `src/lib.rs` `step_bot` | `from_table_with_stats` injection |
| pkcore `src/analysis/player_stats.rs:55,256` | `PlayerStats`, `StatsRegistry` |
| pkcore `src/bot/exploitative_decider.rs:44` | wrapper |
| pkcore `src/bot/exploit.rs:36,197` | `ExploitConfig`, `adjust_profile` |
| pkcore `src/bot/table_snapshot.rs:325` | `from_table_with_stats` |
| `www/` (state renderer) | HUD badges |

---

## Verification

```bash
cargo check --target wasm32-unknown-unknown   # feature builds on wasm (pre-verified)
cargo test                                     # registry + adaptation gates
make build && npx playwright test              # HUD + arena adaptation specs
```

Acceptance: (1) stats accumulate with correct identity attribution;
(2) adaptation provably changes ≥1 decision post-gate and zero decisions
pre-gate / when off; (3) HUD respects `Confidence`; (4) wasm bundle still
builds and the game loop's per-step latency is visually unchanged.

---

## Open Questions

- **Default-on where?** Arena mode (bots-only) is a safe default-on
  showcase; for play mode, adaptive bots raise difficulty — tie the default
  to EPIC-49's difficulty selector?
- **Registry persistence across sessions** via localStorage
  (`StatsRegistry` is serde-able)? Deferred — session-scoped is enough to
  demonstrate adaptation, and cross-session stats raise "it remembers me"
  UX questions.
- **Human stats in the HUD too?** The registry tracks the human seat as well;
  showing the player their own VPIP/PFR is a nice mirror. Cheap once Phase 4
  lands.
