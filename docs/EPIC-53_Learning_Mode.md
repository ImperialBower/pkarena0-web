# EPIC-53: Learning Mode — Drills & Achievements

> **Provenance:** Drafted 2026-07-15 on the `EPIC-50` branch (commit `89205c7`)
> as an alternate vision for EPIC-50, then superseded there by
> [EPIC-50 Decision-Knob Adoption](EPIC-50_Decision_Knob_Adoption.md). Salvaged
> into `docs/` and **renumbered EPIC-50 → EPIC-53** (next free number) on
> 2026-07-18 so the idea isn't stranded on a branch. Code-line references below
> reflect the `main` snapshot at draft time (`55a862c`, pre-EPIC-46 merge) and
> may have shifted — re-verify anchors before implementing.

> **One-line:** A table variant where every hand is a lesson: the app quizzes
> the player on pot odds, outs, equity, ranges, and blockers — graded by the
> same pkcore engine the bots use — and achievements unlock progressively
> harder drills, culminating in the capstone read: *identify which archetype
> (is it the GTO bot?) you've been playing against.*

## Status

| Component | Status |
|---|---|
| Learning-mode session entry + anonymized bot identities | Planned |
| Drill engine (Rust): prompt generation + grading from live table state | Planned |
| Phase 1 drills: pot odds / price / SPR | Planned |
| Phase 2 drills: outs + rule of 2-and-4 | Planned (needs `equity` feature — EPIC-48) |
| Phase 3 drills: equity estimation (HUP + multiway MC) | Planned (needs `equity` feature — EPIC-48) |
| Phase 4 drills: ranges + blockers | Planned |
| Phase 5 capstone: archetype read ("spot the GTO bot") | 🔒 Gated (unlocked by achievements; richest with EPIC-46/47) |
| Achievements ledger + unlock gating (localStorage) | Planned |
| Quiz overlay + achievements gallery UI | Planned |

**Depends on:** [EPIC-46](EPIC-46_Decider_Integration.md) (real archetype
behavior for Phase 5), [EPIC-48](EPIC-48_Real_Equity_WASM.md) (`equity`
feature for Phases 2–3). **Consumes:**
[EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) (HUD stats as read
evidence), [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) (lineup variety).
Phases 0–1 need none of them and can start today.

---

## Context

The app is a single-player NLHE table vs eight bots — every session is
already an unstructured lesson; this EPIC makes the lesson explicit. Where
the code stands on `main` (55a862c, 2026-07-15; note EPIC-46–49
implementation is in flight on the unmerged `EPIC-46` branch at 8a1920b):

- **The game state is fully queryable.** `get_state()` (`src/lib.rs:519`)
  serializes hero cards, board, pot, `to_call`, `min_raise`, legal actions,
  and showdown reveals via `build_game_state()` (`src/lib.rs:1130`) — every
  input a pot-odds or outs drill needs is already in the JSON.
- **A toy version of one drill already ships — as a crutch.** The hero dock
  displays computed `POT ODDS %` and `SPR` (`www/js/main.js:537-547`). It
  gives the answer away; learning mode inverts it into a question.
- **Post-hand review has a complete pipeline.** Every finished hand lands in
  the `COLLECTION` hand history (`src/lib.rs:55,413,426`), exportable via
  `get_session_yaml()` (`src/lib.rs:527`) and replayable step-by-step via
  `replay_snapshot()` (`src/lib.rs:573`, consumed by `www/js/replay.js`).
  Retrospective drills ("what price were you offered on the turn?") can be
  generated from this today.
- **The archetype-read quiz is currently spoiled twice.** Bot profile names
  become seat handles verbatim (`src/lib.rs:97,103-107` →
  `PlayerView.name`, `src/lib.rs:1290`), so `get_state()` returns seats
  literally named `"gto"`, `"maniac"`, `"joker"`. The frontend then adds a
  per-archetype emoji (`www/js/main.js:27-37`) and links each seat name to
  the bot's pkcore YAML config (`www/js/main.js:42`, rendered at
  `www/js/table.js:169-174`). Anonymization is prerequisite work.
- **Grading math is upstream, not enabled.** `Cargo.toml:14` enables only
  `bot-profiles, hand-histories`. Outs, `PotOdds`, `EquityRequest`, and the
  embedded HUP preflop cache all sit behind pkcore's `equity` feature —
  EPIC-48's territory (wasm compile with the full feature set verified
  2026-07-12; in-browser latency budget is its Phase 0). Range drills lean
  on `Playbook`/`PositionRanges` data (pkcore `src/bot/position_ranges.rs:193-412`,
  per EPIC-49's audit).
- **Persistence conventions exist.** localStorage already carries
  `audioEnabled` (`www/js/main.js:25`), `lifetimePnl` (`www/js/main.js:54`),
  and theme keys (`www/js/themes.js:15,29,46`). The achievements ledger adds
  one namespaced key alongside them.
- **No quiz, training, or achievement code exists anywhere** in `src/` or
  `www/` (verified 2026-07-15). This EPIC is net-new surface built on the
  seams above.

**What this EPIC does *not* do:** it does not change bot behavior, betting
logic, or the normal/arena table modes in any way; it does not import
external hands for study (the trainer works on hands you actually play); it
does not add accounts, servers, or telemetry — progress lives in the
browser.

### The kata

- **Things:** `Drill` (a question kind), `DrillPrompt` (a concrete question
  with solution, tolerance, and explanation), `Grade`, `Achievement`,
  `LearningLedger` (persistent progress), `Alias` (an anonymized seat
  identity).
- **Business Requirements:** every prompt is grounded in the live hand,
  never synthetic; grading uses the same pkcore math the bots use; drills
  unlock simplest-first through achievements; bot identities stay hidden in
  learning mode until read correctly.
- **Business Logic:** a `learning` module in the WASM crate that generates
  and grades prompts from table state, plus a thin JS overlay that asks,
  collects, and celebrates.

## Goals

- Every **hero decision point** can become a **drill**: a question about the
  spot the player is actually in, with a graded answer and a one-line
  explanation.
- **Achievements** are the progression system: earning them unlocks harder
  drill kinds — pot odds → outs → equity → ranges/blockers → archetype
  reads. Start simple, get complex.
- The capstone inverts the table: in learning mode the player **doesn't know
  who they're playing** — they earn the reveal by reading behavior,
  including whether a seat is the **GTO bot**.
- **One source of truth for grading:** pkcore's `PotOdds`, `Outs`,
  `EquityRequest`, HUP cache, and `Playbook` — never a parallel JS
  reimplementation.
- Zero impact on normal and arena modes.

## Scope

**In scope:** a learning-mode session entry with anonymized seats; a Rust
drill engine (generation + grading) exported over `wasm_bindgen`; five drill
families (pot odds, outs, equity, ranges/blockers, archetype read); an
achievements ledger in localStorage with unlock gating; quiz overlay and
achievements gallery UI; retrospective drills off the existing hand-history
pipeline; Playwright + cargo test coverage.

**Out of scope:** spaced-repetition scheduling (later); importing external
hand histories to study (later); free-text answers (numeric + multiple
choice only); difficulty-adjusted opponent play while learning (that's
EPIC-49's lever); any persistence beyond localStorage.

---

## Design

### Mode entry & anonymization

```rust
#[wasm_bindgen]
pub fn init_learning_game(rand_seed: f64) -> Result<(), JsValue>
// same setup path as init_game (src/lib.rs:86) plus:
//   LEARNING.set(true);
//   SEAT_PROFILES: Vec<String>   // seat -> real archetype, kept server-side
//                                // of the JSON boundary for grading
// Seat handles become aliases ("Vega", "Rook", ...) so PlayerView.name
// (src/lib.rs:1290) leaks nothing. JS suppresses BOT_EMOJIS + config links
// (main.js:27-44, table.js:169-174) when state.mode == "learning".
```

Rationale: keeping real profile names in a thread_local map (the crate
already runs on five such singletons, `src/lib.rs:47-65`) lets deciders and
grading see the truth while the JSON the frontend renders stays blind. The
alternative — aliasing in JS — leaves the answer sitting in `get_state()`
for anyone who opens the JSON viewer (`www/index.html:99`).

### Drill engine

```rust
pub enum DrillKind { PotOdds, Spr, Outs, RuleOf2And4, EquityHup,
                     EquityMultiway, RangeOpen, Blocker, ArchetypeRead }

#[wasm_bindgen]
pub fn get_drill(unlocked_json: &str) -> String
// Inspects current table state at a hero decision point; picks an
// applicable kind from the caller's unlocked set; stores the solution in a
// LAST_DRILL one-shot (same pattern as LAST_ERROR / LAST_SHOWDOWN,
// src/lib.rs:47-65); returns {kind, prompt, choices?, unit?} JSON.
// Returns {none: reason} when no drill applies (e.g. no draw on board).

#[wasm_bindgen]
pub fn grade_drill(answer_json: &str) -> String
// Grades against LAST_DRILL with per-kind tolerance (pot odds ±2pts,
// equity ±5pts, outs exact). Returns {correct, actual, explanation}.
```

Rationale: generation *and* grading live in Rust so tolerances and math are
kernel truth, testable with plain cargo tests, and immune to JS drift. The
unlocked set is passed *in* because progression state is presentation-side
(localStorage) — the kernel stays pure and stateless across sessions.

Solutions per kind: `PotOdds`/`Spr` from pot arithmetic already in
`GameState` (`src/lib.rs:996-1021`); `Outs`/`RuleOf2And4` from pkcore `Outs`
/ `CaseEvals`; `EquityHup` from the embedded HUP preflop cache;
`EquityMultiway` from seeded-MC `EquityRequest` within the latency budget
EPIC-48 Phase 0 establishes; `RangeOpen`/`Blocker` from
`Playbook`/`PositionRanges` data (pkcore `src/bot/position_ranges.rs:193-412`).

### Archetype read (capstone)

```rust
#[wasm_bindgen]
pub fn guess_archetype(seat: usize, guess: &str) -> String
// {correct: bool, revealed: Option<String>, guesses_used: u32}
// Correct -> seat's real name + emoji restored for the rest of the session.
// Wrong guesses are capped per seat per session (default 2) so the read is
// earned by observation, not enumeration over eight names.
```

The evidence is the game itself: showdown reveals
(`src/lib.rs:314-341,988-994`), the hand log (`www/js/main.js:556`), and —
once EPIC-47 lands — the per-seat HUD stats (VPIP/PFR/AF) as a proper
tell-sheet. The GTO bot is deliberately the hardest read: no exploitable
leak, which is itself the tell.

### Achievements ledger & gating

```rust
// Kernel exposes facts; JS owns the ledger.
// localStorage "learning.v1": { drills: {kind: {asked, correct, streak}},
//                               achievements: [id...], unlocked: [kind...] }
```

| Achievement | Earned by | Unlocks |
|---|---|---|
| Price Checker | 5 correct pot-odds drills | SPR drills |
| Stack Surgeon | 5 correct SPR drills | outs drills |
| Out Counter | 10 correct outs drills | rule-of-2-and-4 drills |
| Quick Math | 5-streak rule-of-2-and-4 | equity drills |
| Coin Flipper | 10 equity answers within ±5pts | range drills |
| Range Rover | 10 correct range drills | blocker drills |
| Card Removal | 5 correct blocker drills | **archetype read mode** |
| Bot Whisperer | first correct archetype read | — |
| Spot the Machine | correctly identify the GTO bot | — |

Rationale for a JS-side ledger: it's presentation/progression state like
`lifetimePnl` (`www/js/main.js:54`), not domain truth; keeping it out of the
WASM crate means no persistence API crosses the kernel boundary.

### Quiz overlay & gallery

A drill overlay mounted like the existing modals (`#settings-overlay`
pattern, `www/index.html:109`): at a hero decision point in learning mode a
non-blocking **"Drill?"** chip appears near the hero dock; accepting pauses
input, poses the prompt, grades, explains, and returns to the action.
Declining costs nothing — learning mode never forces a quiz. The
achievements gallery lives in the settings overlay; unlock toasts reuse the
hand-log status line. Mode selection sits beside the existing New
Game/Arena entry points (`www/js/main.js:418` seeds the session).

---

## Work Items

### Phase 0 — Mode plumbing & anonymization (no pkcore changes)
- [ ] 0a. `LEARNING` flag + `init_learning_game(seed)` sharing the
  `init_game` path (`src/lib.rs:86`); `mode: "learning"` in `get_state()`.
- [ ] 0b. Alias seat handles; `SEAT_PROFILES` thread_local keeps the truth
  (`src/lib.rs:97,103-107`); test: learning-mode `get_state()` JSON contains
  no archetype string.
- [ ] 0c. Frontend: suppress emoji + config links in learning mode
  (`www/js/main.js:27-44`, `www/js/table.js:169-174`); hide the hero-dock
  pot-odds readout (`www/js/main.js:537-547`) — it's now a quiz answer.
- [ ] 0d. `learning.v1` localStorage ledger module + learning-mode entry in
  the UI; Playwright: mode starts, seats show aliases.

### Phase 1 — Pot odds & SPR drills (arithmetic only)
- [ ] 1a. Drill engine skeleton: `get_drill`/`grade_drill` + `LAST_DRILL`
  one-shot; kinds `PotOdds`, `Spr` computed from `GameState` fields
  (`src/lib.rs:996-1021`). Cargo tests: seeded state → known solution;
  tolerance edges (±2pts) both sides.
- [ ] 1b. Drill overlay UI + grade/explanation flow + ledger recording.
- [ ] 1c. Achievements: Price Checker, Stack Surgeon; unlock toast +
  gallery. Playwright: answer 5 drills at seed 0.42 (`tests/helpers.ts:9-16`),
  assert unlock persists across reload.
- [ ] 1d. Retrospective variant: same drills posed from `replay_snapshot`
  steps (`src/lib.rs:573`) in the replay viewer.

### Phase 2 — Outs & rule of 2-and-4 (needs `equity` feature)
- [ ] 2a. Enable pkcore `equity` (coordinate with EPIC-48; wasm compile
  already verified per its Status). Kinds `Outs`, `RuleOf2And4` via
  pkcore `Outs`/`CaseEvals`; drill applicability = hero has a draw.
- [ ] 2b. Cargo tests: flush draw = 9 outs, OESD = 8, gutshot = 4;
  rule-of-2-and-4 grading vs `CaseEvals` truth.
- [ ] 2c. Achievements: Out Counter, Quick Math.

### Phase 3 — Equity estimation
- [ ] 3a. `EquityHup` preflop drills from the embedded HUP cache ("A♠K♠ vs
  a random hand — your equity?").
- [ ] 3b. `EquityMultiway` via seeded-MC `EquityRequest`, iteration budget
  per EPIC-48 Phase 0 findings; run off the hot path (drill time, not
  action time).
- [ ] 3c. Achievement: Coin Flipper. Playwright: equity drill grades within
  tolerance under fixed seed.

### Phase 4 — Ranges & blockers
- [ ] 4a. `RangeOpen` drills against `Playbook`/`PositionRanges` ("does the
  GTO playbook open J♣T♣ from UTG?") — reuse EPIC-49's playbook data for
  all archetypes as it lands.
- [ ] 4b. `Blocker` drills ("which of your cards blocks the nut flush?")
  from hero cards + board texture.
- [ ] 4c. Achievements: Range Rover, Card Removal → unlocks Phase 5.

### Phase 5 — Archetype read (capstone)
- [ ] 5a. `guess_archetype` export with per-seat guess cap; reveal
  restores name + emoji for the session.
- [ ] 5b. Evidence surface: per-seat note chip (hands seen, showdowns
  observed); wire EPIC-47 HUD stats in as they land.
- [ ] 5c. Achievements: Bot Whisperer, Spot the Machine. Playwright: wrong
  guess consumes a cap; correct guess reveals; GTO identification grants
  the achievement.

---

## Key Files

| File | Role |
|---|---|
| `src/lib.rs` (learning module) | mode flag, aliases, drill engine, grading, guess export |
| `www/js/learning.js` (new) | drill overlay, ledger, achievements gallery |
| `www/js/main.js:27-44,418,537-547` | emoji/config suppression, mode entry, dock readout gating |
| `www/js/table.js:169-174` | seat-name rendering (alias-aware) |
| `www/index.html` | drill overlay + gallery markup (modal patterns at `:99,:109`) |
| `www/js/replay.js` | retrospective drill mount |
| `tests/learning.spec.ts` (new) | mode, drills, unlocks, reveal flows |
| pkcore `equity` feature APIs | `Outs`, `CaseEvals`, `PotOdds`, `EquityRequest`, HUP cache |
| pkcore `src/bot/position_ranges.rs:193-412` | range-drill source data |

---

## Verification

```bash
cargo test                      # drill generation + grading (seeded, per kind)
cargo clippy -- -D warnings
make build && npx playwright test tests/learning.spec.ts
npx playwright test             # full suite: normal + arena modes unaffected
```

Acceptance: (1) learning-mode `get_state()` never leaks an archetype name,
emoji, or config link before a correct read; (2) every drill solution is
reproducible from the seeded table state and graded by pkcore math, with
cargo tests per kind; (3) achievements persist across reload and gate drill
kinds exactly per the ladder; (4) declining every drill leaves gameplay
byte-identical to normal mode under the same seed; (5) the pre-existing
Playwright suite still passes untouched.

---

## Open Questions

- **Prompt cadence.** Offer a drill at every eligible decision point, or
  rate-limit (e.g. max one per street) to keep the game feeling like a game?
- **Alias flavor.** Neutral aliases ("Seat 3") vs. table nicknames
  ("Vega", "Rook") — nicknames are friendlier but must never correlate with
  archetype across sessions (shuffle the mapping per session, as the pool
  already is at `src/lib.rs:93-96`).
- **Retrospective-first?** Phase 1d could lead instead of trail: post-hand
  drills don't interrupt play at all, and the replay pipeline is the most
  finished surface in the app. Current ordering favors live drills because
  the decision moment is where the lesson sticks.
- **Wrong-guess penalty.** Cap-per-seat (current design) vs. a chip cost vs.
  a lockout-until-next-showdown. Cap is simplest and can't warp bankroll
  learning.
- **Arena spectator drills.** Arena mode (`src/lib.rs:64,141`) reveals all
  cards — a natural "watch and predict the action" drill family. Deferred;
  worth a sub-letter EPIC (53a) if the live drills prove out.
