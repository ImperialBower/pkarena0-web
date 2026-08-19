# EPIC-55: Coach Mode — Live Play Advisor

> **One-line:** A right-hand drawer that answers the hero's own spot before they
> act — price, equity, chart, and (heads-up on turn/river) a real CFR mixed
> strategy — with every answer stamped by the **tier** that produced it, so a
> heuristic is never dressed up as a solve.

## Status

**Planned — no work started.** Phase 0 exists to close the two open runtime
questions before the design is final.

| Component | Status |
|---|---|
| Phase 0 spike: CFR solve cost in-browser; HUP bundle-size decision | Planned |
| `src/coach.rs` — advice engine (first module split off `lib.rs`) | Planned |
| `get_coach_advice(seed)` wasm export | Planned |
| T0 **Price** — pot odds, breakeven, EV in chips, SPR, position | Planned |
| T1 **Equity** — seeded multi-way Monte Carlo, 9-way | Planned |
| T2 **Chart** — position-aware open/3-bet membership, preflop | Planned |
| T2b **Exact HUP** preflop odds | 🔒 Gated on Phase 0 — costs ~15.8 MB of wasm |
| T3 **Solve** — CFR mixed strategy | 🔒 Gated — heads-up, turn/river only, needs a villain range (pkcore EPIC-39) |
| `#coach-aside` drawer + `www/js/coach.js` | Planned |
| Retire the JS pot-odds/SPR math (`www/js/main.js:572-580`) | Planned |
| Shared-engine seam for [EPIC-53](EPIC-53_Learning_Mode.md) | Planned |

**Depends on:** [EPIC-48](EPIC-48_Real_Equity_WASM.md) (the `equity` feature and
its 500-sample browser budget), [EPIC-50](EPIC-50_Decision_Knob_Adoption.md)
(`EquityMode::Fast` precedent, `src/lib.rs:2203`). T3 additionally depends on
pkcore [EPIC-39](../../pkcore/docs/epics/EPIC-39_Decider_Range_Model.md)
(Planned upstream, **not started**).

**Relates to:** [EPIC-52](EPIC-52_Bot_Decision_Transparency.md) and
[EPIC-53](EPIC-53_Learning_Mode.md) — the three faces of the same theme.
EPIC-52 explains *the bot's* decision; EPIC-53 *hides* the answer and grades a
guess; EPIC-55 *shows* the answer for the hero's own spot. **EPIC-55 builds the
engine the other two consume.**

---

## Context

Every capability shipped in EPIC-46..50 points at the bots. The bots learned
real equity, position-aware ranges, pot-odds discipline, and exploitative
adjustment. The human at seat 0 got none of it. This EPIC turns the engine
around to face the player.

Where the code stands (branch `bump050`, pkcore 0.5.0):

- **The game state is already complete enough.** `build_game_state()`
  (`src/lib.rs:1503`) serializes hero cards, board, pot, `to_call`, `min_raise`,
  `max_bet`, `legal_actions`, stacks, and blinds. Nothing new needs plumbing.
- **pkcore's canonical decision-input type is already in use here.**
  `TableSnapshot::from_table(&table, seat)` (pkcore
  `src/bot/table_snapshot.rs:105,220`) is constructed at `src/lib.rs:1431`
  (`from_table_with_stats`) to feed `decide_seeded` at `:1440`. It carries `pot`, `to_call`, `min_raise`,
  `stacks[]`, `board`, `hole_cards`, `raises_this_street`, and derives
  `.position()` (`:385`) and `.raise_bounds()` (`:548`). **The shortest path to
  a coach endpoint is one more snapshot off the same table.**
- **A parallel math implementation already exists, in JavaScript.** The hero
  dock prints `POT ODDS %` and `SPR` computed inline at
  `www/js/main.js:572-580`. This is the exact drift hazard the EPIC removes:
  pkcore ships `PotOdds` (`src/analysis/pot_odds.rs:24`) and the app does not
  use it.
- **No advisor API exists anywhere in pkcore.** Verified by exhaustive grep of
  `pkcore-0.5.0/src`: no `advise`, `recommend`, `hint`, or `coach` symbol; no
  Chen formula, no Sklansky groups, no push/fold Nash charts, no ICM. Every
  coaching verb in this EPIC is new code in *this* repo, assembled from pkcore
  primitives.
- **`www/js/main.js` is a 1424-line module with no internal boundaries.**
  `table.js` / `replay.js` / `themes.js` show the correct pattern. The coach UI
  goes in `www/js/coach.js`; `main.js` gains exactly one call.

### The hard constraint the tiers exist to respect

pkcore's CFR solver is real, complete, and compiles to `wasm32`
(`src/analysis/gto/` is not feature-gated, `analysis/mod.rs:14`, and contains no
`rayon`). But it answers a **narrower question than the table asks**:

- It is **heads-up only** — `Player::{Oop, Ip}` (`gto/game_tree.rs:133`),
  `Solver::hand_pairs() -> &[(Two, Two)]` (`gto/solver.rs:977`). This table
  seats nine.
- It builds **turn and river subgames only** — `GameTree::build_river`
  (`gto/game_tree.rs:587`) and `build_turn` (`:643`). **There is no flop tree
  and no preflop tree.**
- It requires **both ranges up front** — `SolverConfig { hero_range: Combos,
  villain_range: Combos, board: Board, .. }` (`gto/solver_config.rs:388,433`).
  Nothing in pkcore estimates a villain's range from game state.

So "GTO" is a truthful word for a narrow slice of the spots this app produces,
and a lie everywhere else. The tier ladder below is the mechanism that keeps
the panel honest about which is which.

### The kata

- **Things:** `Tier` (which engine answered), `Advice` (verdict + headline +
  metrics + caveat), `Metric` (a labelled number with its source), `CoachInput`
  (the hero's view of the spot).
- **Business Requirements:** every number the panel shows comes from pkcore, not
  from JavaScript; every answer is stamped with its tier; the coach sees exactly
  what the human sees; the coach never perturbs the game.
- **Business Logic:** a pure, stateless `coach` module in the WASM crate that
  takes a `TableSnapshot` and returns `Advice`, plus a thin JS drawer that
  renders it.

## Goals

- **One source of truth for every number.** `PotOdds`, `Ev`, `EquityRequest`,
  `PositionRanges` — never a JS reimplementation. Retiring
  `www/js/main.js:572-580` is part of the definition of done, not a nice-to-have.
- **Live, before the action.** The panel refreshes whenever
  `phase == "WaitingForHuman"`.
- **Honest tiering.** The panel labels its answer and states its caveat. It
  never calls a 500-sample estimate against random hands a "solve".
- **An engine, not a widget.** `coach.rs` is shaped so EPIC-53's `get_drill` /
  `grade_drill` consume the same solutions instead of recomputing them.
- **Zero impact on play.** No RNG coupling, no added latency in the bot loop, no
  change to arena mode.

## Scope

**In scope:** `src/coach.rs`; the `get_coach_advice` wasm export; tiers T0–T2;
a `#coach-aside` right drawer with a topbar toggle persisted to localStorage;
`www/js/coach.js`; removal of the JS pot-odds/SPR math; Rust unit tests and a
Playwright spec including a card-privacy assertion; a Phase 0 spike whose
findings gate T2b and T3.

**Out of scope:** T3 ships only if Phase 0 and pkcore EPIC-39 both clear —
otherwise it is documented as parked, not attempted. Also out: any advice in
arena (all-bot) mode; multiway CFR of any kind; outs counting (see gap 2);
accounts, servers, or telemetry; changing bot behaviour in any way.

---

## Design

### The tier ladder

Every `Advice` carries the tier that produced it. This is the document's spine.

| Tier | Name | Question | Engine | Seats / streets | Verdict |
|---|---|---|---|---|---|
| **T0** | Price | What am I being offered? | `PotOdds` (`analysis/pot_odds.rs:24,43,61,83`) + `Ev` (`analysis/ev.rs:39,59,121,142`) | 9-way, all streets | **Exact.** Buildable today |
| **T1** | Equity | How often do I win this? | `EquityRequest` → seeded MC, 500 samples (`analysis/equity/spec.rs:92,118`, `engine.rs:68`) | 9-way, all streets | **Estimate.** Buildable today, with a mandatory caveat |
| **T2** | Chart | Is this a standard open / 3-bet from my seat? | `PositionRanges::gto_nine_max()` (`bot/position_ranges.rs:171,265`) | 9-way, **preflop only** | **Exact lookup.** Buildable today |
| **T2b** | HUP | Exactly how does my hand fare heads-up preflop? | `hup_cache::lookup_odds` (`analysis/store/embedded/hup_cache.rs:33`) | Heads-up, preflop | **Exact — but see the size cliff** |
| **T3** | Solve | What is the unexploitable mixed strategy? | `Solver` / `new_turn` → `StrategyProfile::get` → `ActionFrequencies` (`gto/solver.rs:612,667,875`, `strategy_profile.rs:63,349`) | **Heads-up, turn/river only** | **Blocked.** Needs a villain range |

The T0+T1 verdict uses the API that already exists for exactly this comparison —
`PotOdds::is_profitable(equity)` (`analysis/pot_odds.rs:102`) and
`Ev::is_positive()` (`analysis/ev.rs:121`). No new comparison logic is written.

**The T1 caveat is not optional.** `EquityRequest` models every villain as
`PlayerSpec::Random` (`analysis/equity/spec.rs:13`) because nothing estimates a
range. Real ranges are stronger than random hands, so **T1 systematically
overstates hero equity**, and the panel must say so in words. The moment pkcore
EPIC-39 ships, `PlayerSpec::Range` replaces `Random` and the caveat is deleted.

### Module and export

```rust
// src/coach.rs — the crate's first module split off the 4197-line lib.rs.
// Pure and stateless: a snapshot in, an Advice out. No thread_local, no I/O.

pub enum Tier { Price, Equity, Chart, Hup, Solve }

pub struct Metric { label: String, value: String, source: &'static str }

pub struct Advice {
    tier: Tier,
    verdict: String,                          // "Call" | "Fold" | "Raise to $X" | "Mixed"
    headline: String,                         // one plain sentence of why
    metrics: Vec<Metric>,
    frequencies: Option<Vec<(String, f64)>>,  // T3 only
    caveat: Option<String>,                   // e.g. "villains modelled as random hands"
}

pub fn advise(snap: &TableSnapshot, seed: u64) -> Advice;
```

```rust
// src/lib.rs — one new export, following the existing JSON-string convention
// (no #[wasm_bindgen] structs cross the boundary anywhere in this crate).

#[wasm_bindgen]
pub fn get_coach_advice(seed: f64) -> String;    // read-only; never mutates SESSION
```

Rationale for a module rather than more `lib.rs`: `lib.rs` is already 4197 lines
with fourteen inline test modules. The coach is the natural first extraction —
it is pure, it has one job, and EPIC-53 needs to import it. Keeping it inline
would make both problems worse.

### Three invariants — non-negotiable

1. **The coach never reads villain hole cards.** It sees exactly what the human
   sees. Enforced by a Rust test over `advise()` and a Playwright assertion
   modelled on `tests/hand-log-privacy.spec.ts`.
2. **The coach never touches the shared entropy RNG.** EPIC-52 documented the
   trap: the crate holds **one** `SmallRng` in a `thread_local`
   (`src/lib.rs:72`) and `decide_seeded` draws from it at `src/lib.rs:1440`, so
   any shadow computation sharing it would either perturb the game or describe a
   decision that never happened. *(EPIC-52 cites this as `lib.rs:1332`; that
   anchor has since drifted — 1332 is now a `hole_cards` field.)* The coach therefore takes `seed` from JS and always sets
   `EquityOptions.seed` explicitly (`analysis/equity/spec.rs:47`). **Calling
   `get_coach_advice` twice with the same seed must return byte-identical JSON.**
   This is a test.
3. **The coach never blocks the game loop.** It runs only when
   `phase == "WaitingForHuman"`. T3, if it ever ships, is behind an explicit
   button — never automatic.

### UI

- `#coach-aside` — a second `flex:none` right drawer inside `<main id="main-row">`
  (`www/index.html:52`), sibling to `<aside id="log-aside">` (`:57`). Copy its
  rules verbatim from `www/css/layout.css:58-64`, **including** the
  `[hidden] { display:none }` override at line 64 — the id-level `display:flex`
  out-specifies the UA rule and the drawer will not hide without it. Mobile
  becomes a bottom sheet, same as `layout.css:210-219`.
- `#coach-toggle` in `#topbar` beside `#log-toggle` (`www/index.html:47`),
  persisted to `localStorage.coachEnabled` following the `audioEnabled` /
  `lifetimePnl` / theme-key convention (`www/js/main.js:25,54`,
  `www/js/themes.js:15`). Toggle handler mirrors `setLog()`
  (`www/js/main.js:1037`).
- `www/js/coach.js` exports one function, `renderCoach(state, mod)`, called from
  the `WaitingForHuman` branch of `renderState()` — `www/js/main.js:729-732`,
  beside the existing `renderActionButtons(state)`. Nothing else in `main.js`
  changes except the deletion at `:572-580`.
- All colour from `www/css/tokens.css` so the panel works in all four themes.
- The tier is rendered as a visible badge on the panel. That badge is the
  honesty mechanism; it is not decoration and must not be styled away.

---

## Gaps in pkcore

What a coach needs that pkcore 0.5.0 does not have. Recorded here so the
dependency is tracked rather than rediscovered.

| # | Gap | Evidence | Impact |
|---|---|---|---|
| 1 | **No villain-range model.** Every villain is `PlayerSpec::Random`. "What does he have, given he 3-bet the button?" does not exist. | pkcore [EPIC-39](../../pkcore/docs/epics/EPIC-39_Decider_Range_Model.md) — Planned, **zero work started** | **The dominant gap.** Degrades T1 to an over-estimate and blocks T3 and gap 2 outright |
| 2 | **Outs are double-dummy only.** `Outs` is derived from `CaseEvals`, which evaluates *known* holdings against each other. There is no outs-vs-range or outs-vs-random path. | `analysis/outs.rs:10,344`; `analysis/case_evals.rs:36` | **"You have 9 outs" cannot be shown honestly.** Cut from scope, not deferred |
| 3 | **Two `DecisionConfig` knobs are declared but dead.** `outs: Toggle` and `preflop_charts: PreflopCharts { Off, Hup, Solver }` are referenced nowhere outside their own module and the prelude. | `bot/decision_config.rs:33,37,133` | Confirms gaps 1–2 are known upstream; blocked on EPIC-39 |
| 4 | **The solver has no flop or preflop tree.** Only `build_river` and `build_turn`. | `gto/game_tree.rs:587,643` | T3 can never cover flop or preflop spots, even heads-up |
| 5 | **The solver is heads-up only.** No multiway CFR exists, and none is on the pkcore roadmap. | `gto/game_tree.rs:133`; `gto/solver.rs:977`; `gto/mod.rs` module doc | T3 is unreachable at a nine-handed table by construction |
| 6 | **`SolverCache` is absent on wasm** — properly `#[cfg(not(target_arch = "wasm32"))]`, so it is not a compile error, just unavailable. | `gto/mod.rs:132`; `prelude.rs:142` | A browser needs an in-memory cache over `SolverResult::from_binary_bytes` (`gto/solver.rs:282`, filesystem-free). Small, local, buildable |
| 7 | **`RuleBasedDecider` cannot expose frequencies.** `decide_with_rng` is `pub(crate)` and `decide_seeded` returns one *sampled* `PlayerAction`. | `bot/decider.rs:89,100,158` | A "what would a TAG do here?" second opinion can only show a sampled action, never a mixed strategy. Scope it that way or not at all |
| 8 | **The charts are coarse.** Only two action keys exist: `"open_raise"` and `"three_bet"`. No 4-bet, no cold-call, no vs-opener matrices, no postflop nodes. | `bot/position_ranges.rs:87,193,265` | T2 answers "is this a standard open / 3-bet" and nothing beyond |
| 9 | **Three incompatible range representations.** `Combos`/`WeightedCombos` (solver + equity), `WeightedRange` (bot YAML), and `RangeStrategy`'s bare `String`s. They interconvert only by round-tripping through range notation. | `gto/combos.rs:14`; `bot/weighted_range.rs:140`; `bot/range_strategy.rs:36`; the workaround at `bot/decider.rs:553-566` | A coach spanning bot charts *and* the equity engine needs adapter code |
| 10 | **No preflop hand-strength scalar.** No Chen, no Sklansky, no push/fold charts, no ICM — confirmed absent by grep. "Strength" preflop is only ever range membership or an equity number. | exhaustive grep of `pkcore-0.5.0/src` | Any "hand score" the panel shows would be invented here, and should not be |

### The bundle-size cliff (T2b)

`hup_cache` is backed by `include_bytes!(".../generated/hups.bin")`
(`analysis/store/embedded/hup_cache.rs:5`). That file is **15,816,010 bytes**.
The current `www/pkg/pkarena0_web_bg.wasm` is **2,192,632 bytes** — the linker
garbage-collects the table today because nothing references it.

**The first call to `hup_cache::lookup_odds`, `HUPResult::lookup`,
`SortedHeadsUp::hup_result`, or `DealEval::new` with two hands grows the wasm
bundle by roughly 15.8 MB — about 8× its current size.**

T1's Monte Carlo already answers the preflop question to within a fraction of a
percent for a fraction of that cost. **Phase 0 decides:** accept the cliff, fetch
`hups.bin` lazily as a side asset, or drop T2b entirely. The default
recommendation is **drop it** — exactness in a coaching hint is not worth an 8×
download.

---

## Phase 0 — Spike

Answer the two runtime questions before the design hardens. Mirrors EPIC-48
Phase 0, which set the 500-sample budget the same way.

1. Confirm `analysis::gto` links into the `wasm32-unknown-unknown` build with
   the current feature set, and measure the resulting bundle delta.
2. Time one `Solver::new(config).solve()` on a realistic heads-up **river** spot
   and one `new_turn(..)` **turn** spot, sweeping `max_iterations`, against a
   plausible range pair. Record iterations-to-exploitability alongside wall time.
3. Measure the bundle delta from a single `hup_cache::lookup_odds` call.
4. **Decide and record:** live solve vs. precomputed pack via
   `SolverResult::to_binary_bytes` / `from_binary_bytes` (`gto/solver.rs:249,282`);
   and T2b keep / lazy-fetch / drop.

Remove the probe when the phase closes, as EPIC-48 did (`src/lib.rs:266-269`).

## Phase 1 — T0 + T1 panel

The slice that ships standalone value.

- `src/coach.rs` with `Tier`, `Metric`, `Advice`, `advise()`.
- T0: `PotOdds::new(pot, to_call)` → `.breakeven()`; `Ev` → `.as_chips()`; SPR
  and effective stack from `TableSnapshot`; position from `.position()`.
- T1: `EquityRequest` with `PlayerSpec::Exact(hero)` plus one
  `PlayerSpec::Random` per active villain, `EquityOptions { max_samples: 500,
  seed: Some(seed), .. }` — the budget EPIC-48 measured and EPIC-50 adopted
  (`src/lib.rs:2203`).
- Verdict from `PotOdds::is_profitable(equity)`; caveat string always attached.
- `get_coach_advice(seed)` export; `#coach-aside`, `#coach-toggle`,
  `www/js/coach.js`.
- **Delete `www/js/main.js:572-580`** and source the dock's pot-odds/SPR text
  from the coach payload.

## Phase 2 — T2 chart

- `PositionRanges::gto_nine_max().for_position(pos).for_action("open_raise")`
  and `"three_bet"`, plus `Combo::from(Two)` to classify the hero's hand.
- Panel states membership and frequency, and says plainly that only these two
  actions have chart data (gap 8).
- **T2b only if Phase 0 cleared the size cliff.** Borrow the wrapper shape from
  the sibling repo `pkgto-web`, whose `analyze_gto(hero, villain_range)` already
  does this over the same cache.

## Phase 3 — T3 solve *(conditional)*

Ships **only** if Phase 0 shows a workable path **and** pkcore EPIC-39 has
landed a villain-range estimator. Heads-up, turn and river only, behind an
explicit "Solve this spot" button.

- `SolverConfig::new(hero_range, villain_range, board, effective_stack, pot)`
  → `Solver::new` / `new_turn` → `.solve()` → `StrategyProfile::get(node, hand)`
  → `ActionFrequencies` rendered as the mixed strategy.
- `WeightedCombos::after_action` (`gto/weighted_combos.rs:212`) narrows the
  villain range as the hand proceeds.

**If either precondition is unmet, this phase is closed as parked with the
reason recorded.** It is not attempted, and no partial version ships.

## Phase 4 — Shared engine seam

Factor `coach.rs` so EPIC-53's `get_drill` / `grade_drill` call `advise()` for
their solutions rather than recomputing them. Delivers the "one engine, two
faces" decision: Coach shows the answer, Learning hides it and grades a guess.

---

## Verification

- `cargo test --lib coach` — per-tier unit tests over hand-built
  `TableSnapshot`s: pot-odds and EV arithmetic against known values; equity
  bounds; chart membership for a known position/hand pair; correct `Tier` stamp
  on every path.
- **Determinism test:** `advise(&snap, 7)` called twice returns identical
  output, and calling it does not advance the session RNG (assert the next bot
  action is unchanged).
- **Privacy test:** a Rust assertion that no villain hole card appears in the
  `Advice` payload, plus `tests/coach.spec.ts` asserting no villain card text
  renders in `#coach-aside` — modelled on `tests/hand-log-privacy.spec.ts`.
- `tests/coach.spec.ts` — toggle shows/hides the drawer; `localStorage`
  round-trips; the drawer coexists with `#log-aside` at desktop width and
  becomes a bottom sheet under 760px; panel values match `get_coach_advice()`;
  the tier badge is present.
- A latency spec modelled on `tests/strong-equity-latency.spec.ts` — advice must
  not measurably delay the human's turn at Turbo speed. Zero pacing via
  `window.__PK0__.setInstant()` (`www/js/main.js:315`).
- `make ayce` green (clean → build → validate-bots → Playwright).
- Bundle-size check: `www/pkg/pkarena0_web_bg.wasm` must not exceed its 2.19 MB
  baseline by more than the amount Phase 0 sanctioned.
