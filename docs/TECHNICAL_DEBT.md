# Technical Debt

> Maintained by the `/backlog` skill. Items tagged 🤖 were proposed by automated
> review — review and edit them; they are suggestions, not facts. Promote the
> good ones up to **Tracked debt** and delete the rest.

Seeded 2026-07-15. At seeding time the source tree contained **zero**
`TODO`/`FIXME`/`HACK`/`XXX` markers, so there were no code comments to import —
every item below came from reading the code.

## Tracked debt

<!-- Human-authored and code-comment-sourced items. Nothing here yet. -->

_(empty — nothing human-authored yet)_

## 🤖 Automated review findings

Ordered roughly by consequence.

- [x] ~~🤖 **Saved "adaptive bots" preference never reaches WASM on page
  load**~~ — **FIXED 2026-07-15.** `applyAdaptive` moved into `boot()` after the
  modules resolve (and before `restoreFromUrl()`), with a load-guard replacing
  the `?.` no-op so a renamed export now throws loudly. Regression-guarded:
  `get_state()` now surfaces the live `ADAPTIVE` flag (`GameState.adaptive`) and
  `tests/settings.spec.ts` asserts engine and UI agree on load. Was: a top-level
  `applyAdaptive(adaptiveEnabled)` call optional-chained into a silent no-op
  while `_playMod`/`_arenaMod` were still `null`, so games ran adaptive-on
  regardless of the stored preference. (`www/js/main.js`, `src/lib.rs`)

- [ ] 🤖 **No `cargo test` coverage for any `#[wasm_bindgen]` export** — the
  tests drive `PokerSession`/`RuleBasedDecider` directly and never call the
  exported functions, so the phase-guard branching in `human_action`, the
  chip-audit recovery path in `next_hand`, and the YAML entry points have zero
  native coverage — none of which need a wasm target to run. Suggested: add a
  `#[cfg(test)] mod wasm_api_tests` calling the exports directly and asserting
  on returned JSON. (`src/lib.rs:132`, `:186`, `:252`, `:305`, `:596`, `:629`)

- [ ] 🤖 **`next_hand()` strands the session on any non-chip-audit `end_hand()`
  error** — only the `"Chip audit failed"` branch sets `LAST_ERROR` and lets
  play continue; the generic `Err` branch returns an error JSON but leaves
  `PHASE` at `HandComplete` and `LAST_ERROR` unset, so the next poll sees no
  trace and re-attempts `end_hand()` on a possibly-mutated session. Suggested:
  set a terminal phase or persist `LAST_ERROR` on that branch too.
  (`src/lib.rs:423-434`)

- [ ] 🤖 **`init_game` / `init_bot_game` duplicate ~45 lines of bootstrap** —
  profile shuffling, `BotSeat` construction, the `HAND_START_CHIPS` /
  `COLLECTION` / `REGISTRY` / `FORCED_FOLD_COUNT` resets, and table construction
  are copy-pasted, differing only in seat count (8 vs 9) and human-seat
  handling. Any new state cell must be reset in both places or it drifts.
  Suggested: extract a shared session-bootstrap helper both call.
  (`src/lib.rs:132-180` vs `:186-231`)

- [ ] 🤖 **Play/Arena state isolation rests on an undocumented dynamic-import
  trick** — `import('../pkg/pkarena0_web.js?tab=play')` vs `?tab=arena` relies on
  the browser's ES-module cache keying on the full URL to yield two instances
  with independent `thread_local!` state. Nothing comments this invariant; a
  bundler that normalizes query strings would silently collapse both modules
  into one and cross-contaminate Play and Arena. Suggested: comment the import
  site and/or add a startup assertion that the two modules report independent
  state. (`www/js/main.js:292-295`)

- [ ] 🤖 **`www/js/main.js` is a 1294-line monolith with two divergent bot
  loops** — `stepBotsUntilHuman` (play) and `runArena` (arena) each
  re-implement `step_bot()` polling, hand-log formatting, callout dispatch, and
  pacing, already differing in small ways (`runArena` checks `arenaGeneration`
  for cancellation; `stepBotsUntilHuman` has no equivalent guard). The file also
  owns PnL accounting, audio wiring, replay bootstrap, and settings. Suggested:
  factor one parameterized `runBotLoop(mod, {...})` and split the file
  alongside the existing `table.js` / `replay.js` / `cards.js` split.
  (`www/js/main.js:174-201` vs `:1204-1260`)

- [ ] 🤖 **`docs/known-issues.md` "State surface" section is stale** — it lists
  9 `thread_local!` cells at `src/lib.rs:40–55`; the file now has 13 at
  `src/lib.rs:57-93` (the doc's table omits `REGISTRY`, `LAST_SHOWDOWN`,
  `ADAPTIVE`, `FORCED_FOLD_COUNT`). It also cites `init_game` call sites and an
  `audioEnabled`-only `localStorage` in `www/index.html`, but the design2.0
  refactor moved that logic to `www/js/main.js` and added `lifetimePnl` /
  `adaptiveEnabled` keys. This doc is the explicit starting point for the
  deferred mobile-eviction fix, so the stale pointers will misdirect whoever
  picks it up. Suggested: refresh the table and file/line references.
  (`docs/known-issues.md:131-158`)

- [ ] 🤖 **`docs/known-issues.md` forced-fold entry presents fixed work as
  live** — the entry still reads as an open defect with a "Fix direction"
  section, but EPIC-46 landed the repair ladder (`apply_bot_action` clamps an
  under-sized raise up to `min_raise_to()` rather than folding —
  `src/lib.rs:1589-1593`, test at `:1966`). Suggested: strike it through or move
  it under a "Resolved" heading, keeping the note that the true floor is a
  loose bound rather than `=== 0`. (`docs/known-issues.md:10-72`)
