# EPIC-48: Real Equity in the Browser

> **One-line:** Replace the bots' fake equity (hand-rank proxy postflop, coin
> flip preflop) with pkcore's real multi-way `EquityRequest` engine, running
> single-threaded in WASM — delivered through upstream EPIC-36's graded
> `decision:` knobs rather than a forked decider.

## Status

| Component | Status |
|---|---|
| Enable `equity` feature in `Cargo.toml` (wasm compile pre-verified) | Done (`Cargo.toml:17`) |
| Phase 0 spike: `compute(EquityRequest)` runtime behavior in-browser | Done — no panic, rayon serial-fallback confirmed (`equity_probe`) |
| Latency budget: MC sample count vs per-decision wall time (incl. Turbo) | Done — **`fast` = 500 MC samples** (2.8 ms HU / 5.7 ms 4-way) |
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

### Phase 0 — Spike (do first; informs everything) ✅
- [x] 0a. Enable `equity` in `Cargo.toml`; `make build`. (Feature on; wasm
  bundle builds green.)
- [x] 0b. Temporary wasm-exposed probe (`equity_probe` in `src/lib.rs`) runs
  `compute()` for a hero (`AsKs`) vs `seats-1` random villains on a fixed flop
  (`Qd 7h 2c`), forced to Monte Carlo (`exact_threshold: 0`). Driven headless,
  batched, timed with `performance.now()`. **No panic; rayon serial-fallback
  confirmed.** Numbers below.
- [x] 0c. Browser sample budget chosen: **`fast` = 500 MC samples** (see
  "Budget decision" below).

#### 0b results — per-`compute()` latency

Median of 3 batched runs (15 computes each), headless Chromium on the dev
machine (desktop; treat as a **fast-hardware floor** — mid-range mobile is
plausibly 2–5× slower):

| MC samples | heads-up (2) | 4-way |
|---|---:|---:|
| 500 | **2.8 ms** | **5.7 ms** |
| 2 000 | 11.2 ms | 22.7 ms |
| 10 000 | 56.4 ms | 113.4 ms |

Latency is ~linear in samples and ~linear in seat count. Equity estimates were
sane and stable (HU `AsKs` vs random on `Qd7h2c` ≈ 0.56 / 0.55 / 0.54 at
500 / 2 000 / 10 000 — the 500-sample read is within ~2% of the 10k read).

**Structural finding (de-risks the "exact could be seconds serial" worry):**
with unknown villains (`PlayerSpec::Random`) the engine is *never* all-exact, so
it **always** takes the Monte-Carlo path regardless of `exact_threshold`. Exact
enumeration only triggers when every seat has known cards — not the live
decision shape. So the browser doesn't need to *defensively* force `fast`; the
realistic query is MC by construction. (An all-known flop spot would enumerate
`C(45,2)=990` runouts — cheap — so even deliberate exact calls on the flop are
fine; only preflop all-known enumeration would be large, and that still falls
back to MC above the default threshold.)

#### Budget decision (0c)

Adopt **`equity: fast` with 500 MC samples** as the browser default:

- 5.7 ms worst-case here (4-way) leaves headroom under the 10 ms/decision Turbo
  target even at a 2× mobile penalty; ~11 ms at a harsh 2× 4-way is still
  invisible at normal speed (≥1 s/action) and marginal at Turbo (75 ms/action).
- 500 samples already gives ±~2% equity — ample for call/raise gating.
- Bot steps are JS-timer-throttled, so a single ~3–6 ms compute per decision is
  imperceptible. **Caveat to carry into Phase 1:** if one decision issues
  *several* equity calls (e.g. outs + pot-odds + hand strength), keep the
  *combined* budget ≤ ~10 ms — i.e. don't call `compute()` more than ~2× per
  decision at 500 samples, or drop to 250–300 samples if a decision fans out.
- Revisit only if a real device blows the budget → options in Open Questions
  (lower samples per street; equity only on flop+; workers).

> The `equity_probe` export is **temporary** and stays only until Phase 1 wires
> real equity in (blocked on upstream pkcore EPIC-36); remove it then.

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
