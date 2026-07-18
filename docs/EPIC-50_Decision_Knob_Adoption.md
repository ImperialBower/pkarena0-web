# EPIC-50: Decision-Knob Difficulty Adoption

> **One-line:** Now that pkcore **0.3.0** ships graded `decision:` knobs, put them
> to work — add `decision:` blocks to the difficulty bundles so the tiers gain
> **real multi-way equity**, **position-aware ranges**, and **graded pot-odds
> discipline**, retire the interim `strengthen()` shim as *the* strong lever, and
> land EPIC-48's deferred browser-equity phases.

The follow-up EPIC-48 §Phase-1 and EPIC-49 §corrigendum-8 named: "the EPIC-36
`decision:` knobs land in these bundles when the EPIC-48 follow-up opens." This
is that follow-up.

## Status

Status as of branch `EPIC-46` (working tree), 2026-07-17. Phase 0 (dependency)
landed this session; everything else is **Planned**.

| Component | Status |
|---|---|
| pkcore dependency bumped `0.2.1 → 0.3.0`; native + `wasm32` checks pass | **Complete** (`Cargo.toml:14`) |
| `with_decision` helper + `standard_decision()` / `strong_decision()` constructors | **Complete** (`src/lib.rs`) |
| **standard** tier: `ranges: position_aware` (equity/pot_odds at default — see Design revision) | **Complete** |
| **strong** tier: `equity: fast{500}` + `ranges: position_aware`, layered on `strengthen()` | **Complete** |
| **weak** tier: knobs left at default `off` (the proxy floor) — unchanged | **Complete** |
| Regenerate embedded `data/bots/{standard,strong}.yaml` from the code pools | **Complete** |
| Parity gate updated: `bot_bundle_fixture` asserts the `decision:` blocks round-trip | **Complete** (`src/lib.rs` parity tests; suite 20 pass / 0.15 s) |
| Browser equity latency: Playwright Turbo regression with equity on | Planned (Phase 3) |
| Seeded unit test: equity-on out-decides the proxy | Planned (Phase 3) |
| `make bench-tiers`: ordering weak < standard < strong holds with real knobs | Planned (Phase 4) |
| Retire / re-scope `strengthen()` once the bench confirms the knobs carry the tier | Planned (Phase 4) |
| `exploit` knob — **out of scope** as a tier lever (adaptation drags bot-vs-bot; see Scope) | **Deferred** |
| `outs` / `preflop_charts` knobs — **deferred upstream** in pkcore 0.3.0 | **Deferred** |

---

## Context

pkarena0-web drives every bot decision through one `dyn BotDecider::decide_seeded`
call in `step_bot()` (`src/lib.rs:1330-1333`), fed a snapshot built with the
opponent-stats registry attached (`TableSnapshot::from_table_with_stats`,
`src/lib.rs:1323-1327`). Three difficulty tiers — `Difficulty::{Weak, Standard,
Strong}` (`src/lib.rs:121-127`), selected via `set_difficulty` (`src/lib.rs:187`)
and surfaced in `get_state().difficulty` (`src/lib.rs:1195`) — pick a bundle at
lineup-build time via `profiles_for` (`src/lib.rs:1879`).

Each bundle is embedded YAML (`data/bots/{weak,standard,strong}.yaml`,
`include_str!` at `src/lib.rs:1824/1844/1862`), generated from code pools
(`builtin_{weak,standard,strong}_pool`, `src/lib.rs:1906/2043/1943`) and gated by
a parity test that fails the build if YAML drifts from the pool
(`bot_bundle_fixture`, `src/lib.rs:3089-3312`; `make validate-bots`).

**What "strong" is today** is the interim `strengthen()` shim
(`src/lib.rs:1965-2032`): a tight ~10% opening range (`"44+, AJ+, KQ, KJs"`), bluff
frequency clamped `≤8`, `value_threshold: 0.5`, position grades preserved. It is
pure profile-knob tuning with **no equity engine** — measured `+24k chips/100`
over standard (σ≈2k). Its docstring is explicit that it is "interim until upstream
pkcore EPIC-36 ships real capability knobs" (`src/lib.rs:1951-1964`).

Three facts make this EPIC ready to open:

1. **Upstream shipped.** pkcore 0.3.0 wires `equity` / `ranges` / `pot_odds` /
   `exploit` into `RuleBasedDecider`; the dependency is pinned (`Cargo.toml:14`)
   and both native and `wasm32-unknown-unknown` compile clean against it.
2. **The bundles are already shaped to receive knobs.** `BotBundle` is
   `{ name, profiles }` (`src/lib.rs:61-66`); adding `decision:` to each profile
   is a serde-default field on pkcore's `BotProfile` — absent blocks are
   unchanged (EPIC-49 corrigendum §8). No `data/bots/*.yaml` carries `decision:`
   yet (grep-confirmed).
3. **The dormant data is waiting.** `attach_archetype_playbook`
   (`src/lib.rs:2062+`) already carries per-position ranges "as data for upstream
   pkcore EPIC-36 (`ranges: position_aware`)" (`src/lib.rs:2057-2061`) — the
   decider consults positional *betting* today but not positional *ranges*.
   Turning on `ranges: position_aware` activates ranges already shipped.

EPIC-48 Phase 0 already de-risked the browser side: `equity` is enabled
(`Cargo.toml:17`), the engine runs in-browser with no panic (rayon serial-fallback
on threadless wasm), and the latency budget is set — **`fast` = 500 MC samples =
2.8 ms HU / 5.7 ms 4-way**, comfortably under the 10 ms/decision Turbo target
(EPIC-48 §Phase-0, latency table lines 136-140).

### This EPIC does **not**

- Adopt **`outs`** or **`preflop_charts`** — both are *deferred upstream* in
  pkcore 0.3.0 (they need villain cards the decider never sees). This overrides
  EPIC-48 Phase-1b's original `outs: on`. `three_bet`/`call_three_bet` also stay
  dormant upstream (EPIC-49 corrigendum §8).
- Make **`exploit`** a tier lever. EPIC-49 corrigendum §1 measured adaptation as a
  consistent *drag* in bot-vs-bot benches (−2.7k / −3.8k chips/100); its value is
  modeling *human* tendencies, invisible to a bot-vs-bot bench. Adaptation stays
  the existing user toggle (`ExploitativeDecider::wrap_with_config`,
  `src/lib.rs:1710-1725`, gated by `effective_adaptive`, `src/lib.rs:1893`).
  Expressing it as the pkcore `decision.exploit` knob instead of the wrapper is a
  possible follow-on, not this EPIC.
- Introduce new archetypes, new tiers, or a seeded deck. Deals stay entropy-RNG;
  the benches stay statistical with σ-margins (see Test Plan).

---

## Goals

- Add a **`decision:`** block to the `standard` and `strong` bundle profiles that
  turns on **real equity**, **position-aware ranges**, and **graded pot-odds
  discipline**, defaulting the `weak` tier to the off floor.
- Land EPIC-48's deferred **browser-equity** phases: prove equity-on decisions
  beat the proxy and that Turbo latency stays within budget.
- Preserve the **difficulty ordering** weak < standard < strong under
  `make bench-tiers`, now driven by real capabilities rather than the
  `strengthen()` shim.
- Keep the **parity gate** honest: the embedded YAML must round-trip to the code
  pools *including* the new `decision:` blocks.
- Keep every existing bundle-less / `decision:`-less path **behavior-identical**.

## Scope

**In scope:** `decision:` blocks on `standard`/`strong` pools; regenerating the
embedded YAML; updating `bot_bundle_fixture` parity assertions; a seeded
equity-on-vs-off decision unit test; a Playwright Turbo latency regression;
re-validating `make bench-tiers`; deciding the fate of `strengthen()` once the
knobs carry the tier.

**Out of scope:** `outs`, `preflop_charts` (deferred upstream); `exploit` as a
tier lever (kept as the user adaptivity toggle); `three_bet`/`call_three_bet`
activation (dormant upstream); new archetypes; a seeded deck.

**Rules the feature must obey:**

- A profile with no `decision:` block plays exactly as it does today (pkcore's
  serde default is the historical proxy/flat/strict-discipline floor).
- `weak` never gains capability knobs — it is the floor by construction.
- Postflop equity must cost **≤ 1 `compute()` call per decision** at
  `fast`/500 samples, keeping the per-decision budget under ~10 ms.
- The bench ordering weak < standard < strong must survive with a ≥5σ margin.

---

## Design

### `decision:` blocks per tier

The knobs attach in the code pools (the single source of truth); the YAML is
regenerated from them, so the parity gate keeps both in lockstep.

**weak** — no `decision:` block. pkcore's `DecisionConfig::default()` is the
historical floor (proxy equity, flat ranges, strict-but-proxy pot odds). Left
explicit-off by omission; `weaken()` (`src/lib.rs:1927-1936`) is unchanged.

**standard** — activate the dormant position-aware ranges only (cheap; no MC
engine on the default in-browser path):

```yaml
decision:
  ranges: position_aware   # activates attach_archetype_playbook's dormant ranges
  # equity stays off (proxy); pot_odds stays at the strict default (1.0)
```

**strong** — real equity + position-aware ranges, layered on the existing
`strengthen()` base (tight range + bluff clamp + `value_threshold: 0.5`):

```yaml
decision:
  equity: { mode: fast, samples: 500 }   # EPIC-48 Phase-0 browser budget
  ranges: position_aware
  # pot_odds stays at the strict default (1.0)
```

> **Design revision (landed during implementation).** The original sketch put
> `equity: fast` on *both* tiers and graded `pot_odds` discipline (0.75 / 1.0).
> Implementation falsified that shape:
> 1. **Equity belongs to the strong tier only.** Putting the MC engine on the
>    *default* (standard) tier ran `compute()` on every postflop decision of the
>    default in-browser path *and* made the native suite pathological — one
>    EPIC-49 positional test (`btn_and_utg_decisions_diverge_for_each_archetype`,
>    512 seeds × 2 positions × 2 spots × 7 archetypes) drove the full `cargo test
>    --lib` from ~0.2 s to **701 s** and confounded its positional-divergence
>    assertion (position-independent equity washed out the positional-betting
>    nuance the test measures). Reserving equity for `strong` keeps standard cheap
>    and keeps EPIC-49's tests fast and meaningful.
> 2. **`pot_odds` discipline is not a useful gradient here.** The historical
>    default is already `1.0` (strict break-even); *lowering* standard to `0.75`
>    would make it call looser — i.e. *weaker*, not stronger. Both tiers keep the
>    strict default.
>
> Net tiers: **weak** (proxy, flat, from `weaken()`) < **standard** (proxy,
> `position_aware` ranges) < **strong** (real equity, `position_aware`, on the
> tighter `strengthen()` base). Strong owns the equity engine; standard's lift is
> the cheap, always-affordable position-range activation.

> **Open tuning question (Phase 4).** With `ranges: position_aware` + `equity`
> live on strong, `strengthen()`'s *manual* range-tightening may be redundant
> with, or fight, the position-aware ranges. The bench decides: if the knobs
> preserve `strong > standard` with margin, `strengthen()` is reduced to the
> bluff-clamp + `value_threshold` (or retired). We do **not** guess — we measure.

### Where the knobs attach

`decision:` is set in the pool builders, not hand-edited in YAML:

```rust
// builtin_standard_pool() / builtin_strong_pool() — src/lib.rs:2043 / 1943
fn with_decision(mut p: BotProfile, cfg: DecisionConfig) -> BotProfile {
    p.decision = cfg;      // pkcore 0.3.0 field, serde(default)
    p
}
```

The `#[ignore]`d fixture generators (`generate_{standard,strong}_bundle`,
`src/lib.rs:3097/3214`) then re-emit the YAML via `std::fs::write`, and the parity
tests assert the embedded YAML deserializes back to the pool *including* the new
field (`BotProfile: PartialEq` already compares `decision`).

### Browser equity (EPIC-48 Phases 1-2, folded in here)

No new wasm plumbing is required — `equity` is already enabled and proven in
`src/lib.rs`'s wasm build. Turning on `equity: fast` in the bundles is what makes
`step_bot()`'s `decide_seeded` call actually invoke `compute()`. The remaining
work is *validation*: a seeded unit test that an equity-on profile out-decides the
proxy (EPIC-48 Phase-2a), and a Playwright Turbo regression that a 20-hand arena
still completes within spec timeouts with equity on (Phase-2b, using
`window.__PK0__.setInstant()`, `www/js/main.js:298`).

### The `exploit` knob — deliberately not a tier lever

The `exploit` prerequisite is already wired end-to-end (registry attached at
`src/lib.rs:1323-1327`; `ExploitativeDecider` wrapper in `make_bot_seat`). But
EPIC-49 corrigendum §1 proved adaptation *drags* bot-vs-bot chips/100. So EPIC-50
leaves adaptation as the user toggle and keeps `decision.exploit` at `off` in all
bundles. This is a deliberate honesty choice: the tier bench can only measure what
bot-vs-bot play reveals, and it cannot reward opponent-modeling of a human.

---

## Work Items

### Phase 0 — Prerequisite: consume pkcore 0.3.0
- [x] 0a. Bump `pkcore = "0.3.0"` in `Cargo.toml:14`; refresh `Cargo.lock`.
- [x] 0b. Confirm native (`cargo check --tests`) and `wasm32-unknown-unknown`
  (`cargo check --target wasm32-unknown-unknown`) both compile clean.

### Phase 1 — Schema flow-through
- [x] 1a. Import `DecisionConfig`/`EquityMode`/`RangeMode` from the pinned pkcore
  in `src/lib.rs`.
- [x] 1b. Add a `with_decision(profile, cfg)` helper + `standard_decision()` /
  `strong_decision()` constructors; thread into the standard/strong pool builders
  (`src/lib.rs`), default-off elsewhere.

### Phase 2 — Bundle adoption
- [x] 2a. **standard** pool: `ranges: position_aware` (equity/pot_odds at default
  — see Design revision; equity is strong-only).
- [x] 2b. **strong** pool: `equity: fast{500}` + `ranges: position_aware`, layered
  on the `strengthen()` base.
- [x] 2c. Regenerate `data/bots/{standard,strong}.yaml` via the `#[ignore]`d
  fixture generators; `decision:` blocks present (8 each, joker excluded),
  `weak.yaml` unchanged (0 blocks).
- [x] 2d. Extend `bot_bundle_fixture` parity tests with explicit per-tier
  `assert_eq!` on `decision` (standard carries `standard_decision()`, strong
  `strong_decision()`, joker default).

### Phase 3 — Browser-equity validation (EPIC-48 Phases 1-2)
- [ ] 3a. Seeded unit test: an `equity: exact/fast` profile makes a demonstrably
  better decision than the proxy on an authored `TableSnapshot` (fold a dominated
  hand vs a shove the proxy would call). Authored snapshot → deterministic.
- [ ] 3b. Playwright: a 20-hand Turbo arena run with equity on completes within
  the existing `arena.spec.ts` timeouts (`setInstant()` zeroes JS pacing).
- [ ] 3c. Sanity: `≤ 1 compute()` per decision at 500 samples (assert on a decision
  trace or reason from the single `hand_equity` call site).

### Phase 4 — Ordering honesty & `strengthen()` fate
- [ ] 4a. Run `make bench-tiers`; confirm weak < standard < strong holds with the
  real knobs, ≥5σ margin. Record the measured chips/100 deltas.
- [ ] 4b. Decide `strengthen()`'s fate from the data: keep, reduce to
  bluff-clamp + `value_threshold`, or retire in favor of `ranges: position_aware`
  + strict `pot_odds`. Update its docstring (`src/lib.rs:1951-1964`) to drop the
  "interim until EPIC-36" language either way.
- [ ] 4c. Update EPICS.md / EPIC-48 / EPIC-49 status rows to point at EPIC-50 as
  the landed adoption.

---

## Test Plan

| Test | Asserts |
|---|---|
| `bot_bundle_fixture::standard_bundle_matches_default_pool` (extended) | `standard.yaml` round-trips to the pool **with** its `decision:` block |
| `bot_bundle_fixture::strong_bundle_matches_strengthened_pool` (extended) | `strong.yaml` decision block = strict knobs; still `bluff < 9`, playbook present |
| new `equity_on_outdecides_proxy` (authored snapshot, seeded decider RNG) | equity-on correctly plays a spot the proxy misplays (fold a dominated hand vs a shove the proxy calls) — deterministic, no real deal |
| `difficulty_ordering_tests::standard_tier_beats_weak_tier` | still `standard_net > weak_net` with knobs on (≥5σ) |
| `difficulty_ordering_tests::strong_tier_beats_standard_tier` | still `strong_net > standard_net` with knobs on (≥5σ) |
| `tests/arena.spec.ts` (Turbo, equity on) | 20-hand run completes within existing timeouts |

Determinism note: native benches cannot seed the deck (pkcore shuffles from an
entropy thread-local RNG), so behavioral tests use **authored `TableSnapshot`s**
and the tier benches stay **statistical with σ-margin**, `#[ignore]`d out of the
fast suite (mirrors EPIC-49 §3). The bench seeds only the decider RNG
(`SmallRng::seed_from_u64(0xEC49)`, `src/lib.rs:3390`).

## Key Files

| File | Role |
|---|---|
| `Cargo.toml:14` | pkcore `0.3.0` pin (Phase 0, done) |
| `src/lib.rs:2043` / `:1943` | `builtin_standard_pool` / `builtin_strong_pool` — where `decision:` attaches |
| `src/lib.rs:1965-2032` | `strengthen()` — the interim shim to re-scope in Phase 4 |
| `src/lib.rs:2062+` | `attach_archetype_playbook` — carries the position ranges `ranges: position_aware` activates |
| `src/lib.rs:1330-1333` | the `decide_seeded` call site the knobs flow through |
| `src/lib.rs:3089-3312` | `bot_bundle_fixture` parity gate (`make validate-bots`) |
| `src/lib.rs:3315-3558` | `difficulty_ordering_tests` (`make bench-tiers`) |
| `data/bots/{standard,strong}.yaml` | regenerated bundles gaining `decision:` blocks |
| `tests/arena.spec.ts` | Turbo latency regression with equity on |

## Reuse (do NOT recreate)

- pkcore `DecisionConfig` + `EquityMode`/`RangeMode`/`PotOddsConfig` — the whole
  schema; do not re-model knobs locally.
- `attach_archetype_playbook` position ranges — already authored; just activate.
- `run_matchup` (`src/lib.rs:3371`) cash-mode-reset bench harness — reuse as-is
  for Phase 4.
- The `#[ignore]`d fixture generators (`src/lib.rs:3097/3214`) — reuse to
  regenerate YAML; do not hand-edit bundle files.
- `ExploitativeDecider` toggle path — leave intact; do not replace with the
  `exploit` knob in this EPIC.

## Compatibility

- Bundles/profiles without a `decision:` block are byte-identical and
  behavior-identical (pkcore serde default + `skip_serializing_if`). The `weak`
  tier and any external profile are untouched.
- `get_state().difficulty` and `set_difficulty` are unchanged — the tiers gain
  capability internally; the JS surface is stable.
- Latency stays within the EPIC-48 Phase-0 budget (500 samples, ≤1 `compute()`
  per decision).

## Dependencies

- **Built on:** pkcore **EPIC-36** (shipped in 0.3.0) — the knobs themselves;
  pkarena0-web **EPIC-48** (browser-equity Phase 0) and **EPIC-49** (the bundles
  and the tier bench).
- **Unblocks:** the "strong bundle consumes EPIC-36 knobs" row deferred in
  EPIC-49 (§21) and EPIC-48 Phases 1-2.
- **Related:** **EPIC-47** (stats plumbing) — its registry is the `exploit`
  prerequisite, deliberately not used as a tier lever here.

## Verification

```bash
# Phase 0 (done): consumes pkcore 0.3.0 on both targets.
cargo check --tests
cargo check --target wasm32-unknown-unknown

# Parity gate: embedded YAML (incl. decision: blocks) round-trips to the pools.
make validate-bots            # cargo test --lib bot_bundle

# Browser-equity decision test (authored snapshot, deterministic).
cargo test --lib equity_on_outdecides_proxy

# Difficulty ordering with the real knobs (release, statistical, ~15s).
make bench-tiers              # cargo test --release --lib difficulty_ordering -- --ignored --nocapture

# Turbo latency regression with equity on.
make build && npx playwright test arena.spec.ts
```

Acceptance: (1) `weak`/`decision:`-less profiles are behavior-identical; (2) the
parity gate passes with `decision:` blocks embedded; (3) an equity-on profile
provably out-decides the proxy in a seeded unit test; (4) `make bench-tiers` keeps
weak < standard < strong at ≥5σ with the knobs carrying the tier; (5) a 20-hand
Turbo arena with equity on stays within existing Playwright timeouts; (6) no
`outs` / `preflop_charts` / tier-lever `exploit` adopted (all deferred, per Scope).
