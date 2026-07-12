# EPIC-48: Real Equity in the Browser

> **One-line:** Replace the bots' fake equity (hand-rank proxy postflop, coin
> flip preflop) with pkcore's real multi-way `EquityRequest` engine, running
> single-threaded in WASM — delivered through upstream EPIC-36's graded
> `decision:` knobs rather than a forked decider.

## Status

| Component | Status |
|---|---|
| Enable `equity` feature in `Cargo.toml` (wasm compile pre-verified) | Planned |
| Phase 0 spike: `compute(EquityRequest)` runtime behavior in-browser | Planned |
| Latency budget: MC sample count vs per-decision wall time (incl. Turbo) | Planned |
| Upstream: pkcore EPIC-36 `DecisionConfig` (`decision:` YAML knobs) | **Blocked — Planned upstream** |
| Adopt `decision: { equity: fast, outs: on, pot_odds: … }` profiles | Planned (post EPIC-36) |
| Embedded HUP preflop odds (`hup_cache::lookup_odds`, wasm-safe) evaluation | Planned |
| Playwright: game-speed regression at Turbo with equity on | Planned |

**Depends on:** [EPIC-46](EPIC-46_Decider_Integration.md);
pkcore [EPIC-36](../../pkcore/docs/EPIC-36_Configurable_Bot_Capabilities.md)
(Planned upstream) for the knob wiring.
**Relates to:** [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) consumes the same
knobs as difficulty levers.

---

## Context

What the bots currently believe about their hands
(pkcore `src/bot/decider.rs:447-472`, `hand_equity()`):

- **Preflop:** a weighted coin flip. The hand's frequency in the profile's
  `open_raise` range is used as a probability of returning `1.0`, else `0.0`.
  `JJ:0.7` is "the nuts" 70% of the time and "trash" 30% — a mixed strategy
  masquerading as equity. Hands outside the range are always `0.0`.
- **Postflop:** `1.0 - hand_rank_value / 7462` — absolute hand strength vs a
  uniform random holding, blind to opponent count, ranges, and draws. A pair
  on a four-flush board scores the same as on a rainbow one.

Meanwhile pkcore ships (behind the un-enabled `equity` feature):

- `analysis::equity::engine::compute(&EquityRequest) -> EquityReport`
  (`engine.rs:68`) — auto-selects **exact enumeration** (work ≤
  `exact_threshold`) or **seeded Monte Carlo** (capped at `max_samples`).
- `EquityRequest` (`spec.rs:92`) with `PlayerSpec::{Exact, Range, Random}`
  (`spec.rs:13`) — equity vs exact hands, vs ranges, or vs random; 2–10 seats.
- `EquityOptions { exact_threshold, max_samples, seed }` (`spec.rs:47`) —
  deterministic, budget-bounded.
- `Outs` / `CaseEvals` (`src/analysis/outs.rs:10`, `case_evals.rs:24`) for
  draw equity, and `PotOdds` (`pot_odds.rs:24`) already consumed by the
  decider's call gates — which are simply being fed a bad estimate today.

### WASM viability — what's verified vs open

**Verified (2026-07-12):** pkcore 0.2.1 compiles cleanly for
`wasm32-unknown-unknown` with
`--no-default-features --features bot-profiles,hand-histories,equity,player-stats`
(1m05s check). The `store` feature's native-only deps (`rusqlite`, `zstd`)
are target-gated and irrelevant here.

**Open (Phase 0 spike):** the engine parallelizes with rayon
(`engine.rs:22`), unconditionally. On threadless wasm, modern rayon-core
falls back to serial execution rather than spawning workers — but this must
be *proven in-browser*, not assumed: run `compute()` on a 4-way flop spot in
the actual bundle and confirm (a) no panic, (b) acceptable latency.
Exact enumeration that is instant on 10 native cores may be seconds serial —
so the browser default must be `Fast` (Monte Carlo) with a tuned sample
budget, not `Exact`.

### Delivery vehicle: upstream EPIC-36, not a fork

pkcore EPIC-36 ("Configurable Bot Capabilities", **Planned** upstream) wires
the real engine into `RuleBasedDecider` behind graded `decision:` YAML knobs
(`equity: off|fast|exact`, `outs`, `pot_odds` discipline, `ranges`,
`preflop_charts`), defaulting to today's behavior. Duplicating that wiring in
a web-side custom decider would be waste and drift. This EPIC therefore:

1. does the wasm-specific groundwork now (feature flag, runtime spike,
   latency budget, embedded-HUP evaluation),
2. adopts the knobs the moment EPIC-36 ships,
3. feeds wasm constraints back upstream (e.g. "browser profiles must default
   `equity: fast` with ≤N samples"; "`preflop_charts: hup` must resolve via
   the wasm-safe embedded `hup_cache::lookup_odds`
   (`src/analysis/store/embedded/hup_cache.rs:33`), not the SQLite path").

---

## Goals

- Bots price calls/raises with real multi-way equity and draw awareness.
- Per-decision compute stays imperceptible at normal speed and acceptable at
  Turbo (Playwright specs already require Turbo for multi-hand runs).
- Deterministic under seed (MC uses `EquityOptions.seed`).
- All equity behavior reachable purely from profile YAML (EPIC-36 knobs) —
  no web-side decision-logic fork.

## Scope

**In scope:** `equity` feature enablement; in-browser runtime spike and
latency benchmarking; sample-budget tuning; EPIC-36 adoption; embedded HUP
preflop lookup evaluation; upstream feedback/PRs for wasm-specific gaps.

**Out of scope:** the CFR `Solver` at runtime (explicitly ruled out upstream —
too slow for live play); `preflop_charts: solver` until offline-generated
charts exist; web workers / threaded wasm (revisit only if serial MC can't
meet the latency budget).

---

## Work Items

### Phase 0 — Spike (do first; informs everything)
- [ ] 0a. Enable `equity` in `Cargo.toml`; `make build`.
- [ ] 0b. Temporary wasm-exposed probe: run `compute()` for a heads-up and a
  4-way flop spot at samples ∈ {500, 2000, 10000}; log wall-time via
  `performance.now()`. Confirm serial-fallback (no panic) and record numbers
  in this doc.
- [ ] 0c. Decide the browser sample budget (target: ≤10ms per decision at
  Turbo; bot steps are JS-timer-driven so even ~50ms may be invisible at
  normal speed — measure, don't guess).

### Phase 1 — Upstream adoption (blocked on pkcore EPIC-36)
- [ ] 1a. Track EPIC-36; review its `DecisionConfig` schema against wasm
  constraints while it's still in design (cheapest time to influence).
- [ ] 1b. On release: bump pkcore, add `decision:` sections to the bot
  profiles (via EPIC-49's YAML lineup), default `equity: { mode: fast,
  samples: <Phase-0 budget> }`, `outs: on`.
- [ ] 1c. If EPIC-36's `preflop_charts: hup` resolves via the native SQLite
  path only, propose/PR the embedded `hup_cache` fallback for wasm.

### Phase 2 — Validation
- [ ] 2a. Seeded unit test: equity-on profile makes a demonstrably better
  decision than equity-off in a constructed spot (e.g. folds a dominated
  hand facing a shove that the proxy would call).
- [ ] 2b. Playwright Turbo regression: 20-hand arena run completes within
  existing spec timeouts with equity on.
- [ ] 2c. Arena chips/100 comparison (EPIC-49 harness): equity-on lineup
  beats equity-off lineup over a seeded long run.

---

## Key Files

| File | Role |
|---|---|
| `Cargo.toml` | add `equity` feature |
| `src/lib.rs` (spike probe, later removed) | Phase 0 latency measurements |
| pkcore `src/analysis/equity/{engine,spec,result}.rs` | the engine |
| pkcore `src/bot/decider.rs:447-472` | the proxy being replaced (upstream) |
| pkcore `docs/EPIC-36_Configurable_Bot_Capabilities.md` | upstream design |
| pkcore `src/analysis/store/embedded/hup_cache.rs:33` | wasm-safe preflop odds |
| `data/bots/*.yaml` (EPIC-49) | where `decision:` knobs land |

---

## Verification

```bash
cargo check --target wasm32-unknown-unknown        # already green with equity
make build                                          # bundle builds
npx playwright test                                 # Turbo latency regression
```

Acceptance: (1) spike numbers recorded (no panic, budget chosen);
(2) equity-driven decisions differ from proxy decisions in constructed spots
and win the seeded arena comparison; (3) no Playwright timeout regressions
at Turbo; (4) zero web-side forks of decider logic.

---

## Open Questions

- **Villain modeling.** EPIC-36 starts villains as `PlayerSpec::Random`; a
  later upgrade to `PlayerSpec::Range` (fed by the villain archetype's own
  open range, or EPIC-47 observed stats) is where real strength lives —
  upstream question, tracked here.
- **Caching.** EPIC-36 plans `(hole, board)` memoization; in the browser the
  same memo works per-hand. Is per-session caching worth it given MC is
  already budgeted? Likely no — measure first.
- **If serial MC blows the budget:** options are (a) lower samples per
  street, (b) equity only on flop+ (preflop uses embedded HUP), (c) web
  workers. Pick after Phase 0 data.
