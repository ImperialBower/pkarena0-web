# Prefold (pre-action Check/Fold) — Design

**Date:** 2026-07-18
**Status:** Approved, pending implementation plan
**Scope:** Play mode only. Client-side (`www/js/main.js` + CSS). No Rust/WASM change.

## Context

In Play mode, seat 0 is the human; the other seats are bots. When the bots
are acting between the human's decisions, the `#action-buttons` area is empty
and the human waits. The human's action buttons (including **Fold**) only
appear when it becomes their turn (`SessionPhase::WaitingForHuman`).

The engine (`src/lib.rs:1788` `derive_legal_actions`) only offers **Fold**
when a bet faces the hero (`to_call > 0`). When the hero can check for free
(`to_call == 0`), Fold is not a legal action — folding a hand you can check
for free is never rational.

This feature adds an online-poker-style **pre-action Check/Fold** control: a
single toggle button that appears *while the bots are still acting*, letting
the human commit in advance to folding (or checking) on their next turn.

## Goal

Let the human arm an automatic **Check/Fold** before their turn arrives so
they don't have to wait and click. When armed:

- facing a bet → auto **Fold**
- can check for free → auto **Check**

## Non-goals

- No persistent/settings auto-fold (this is per-decision, not a standing
  preference). It is **not** persisted to `localStorage`.
- No engine (`lib.rs`/WASM) changes. All required data is already in the
  `GameState` JSON.
- Arena mode is unaffected (no human, no action buttons there).

## Decisions (locked during brainstorming)

1. **Timing:** pre-action — the control appears during the bots-acting window,
   before the human's turn. (Not a persistent Settings toggle.)
2. **No-bet semantics:** Check/Fold — check when checking is free, fold when
   facing a bet.
3. **Control style:** a single arm-able **Check/Fold** toggle button (click to
   arm/disarm, armed = highlighted). No separate checkbox.

## Approach (A — pure client-side)

Everything lives in the front end. A module-level flag tracks the armed state;
the toggle renders in the existing `#action-buttons` area during the
bots-acting window; when the human's turn arrives and the flag is set, the
existing `onHumanAction()` path is invoked automatically for Check or Fold and
the normal button render is skipped.

This rides the existing control flow: seat 0 acting always routes back through
`stepBotsUntilHuman()`, so re-showing the toggle on each new street is free —
`onHumanAction()` already ends by calling that loop again.

Approaches B (engine-side pre-commit) and C (hybrid) were considered and
rejected: B duplicates the check/fold logic across the WASM boundary and
requires a rebuild for a pure UI convenience; C is just A with a helper (the
helper is included in A anyway).

## Behavior & lifecycle

- **When the toggle shows:** during `stepBotsUntilHuman()`, whenever the human
  (seat 0) is still in the hand and still has actions coming (not folded, not
  all-in, hand not over). It renders in the same `#action-buttons` spot where
  the real Fold button normally appears.
- **If the human is first to act** (no bots act before them) there is no
  pre-action window; the normal action buttons appear as today.
- **Arming:** clicking the toggle flips armed ⇄ disarmed. Armed = highlighted /
  "lit". The button carries `data-act="prefold"` for Playwright tests.
- **When the human's turn arrives and armed:**
  - `legal_actions` includes `Fold` → auto **Fold** (`onHumanAction('Fold', 0, state)`)
  - else `legal_actions` includes `Check` → auto **Check** (`onHumanAction('Check', 0, state)`)
  - else (e.g. hero all-in, empty `legal_actions`) → do nothing; render the
    normal buttons.
- **One-shot per decision:** after it fires, it **disarms**. If the fire was a
  Check and the hand continues to the next street, the (now disarmed) toggle
  re-appears when the bots act again; the human re-arms if they still want out.
  This keeps the human in control rather than silently checking down through
  every street.
- **Resets:** `preFoldArmed` clears on a new hand (new `hand_number` detected)
  and after any fire. It is never written to `localStorage`.

## Code touch-points

`www/js/main.js`:

- New module var `preFoldArmed = false` near the other UI state (~lines 12–59).
- New `renderPreAction(state)` — renders the single arm-able toggle button into
  `#action-buttons`; its click handler flips `preFoldArmed` and re-renders the
  toggle's lit state.
- New helper `autoActFold(state)` — encapsulates the check/fold decision above;
  returns whether it fired. Called from the `WaitingForHuman` entry point.
- Hook `stepBotsUntilHuman()` (~line 174) to call `renderPreAction(state)`
  while the human is still in the hand and it is not yet their turn.
- Hook the `WaitingForHuman` path (`renderState()` ~line 722 and/or the top of
  `renderActionButtons()` ~line 741): if `preFoldArmed`, call `autoActFold` and
  skip the normal render; otherwise render normally and clear any pre-action
  toggle.
- Reset `preFoldArmed = false` at new-hand detection and after each fire.

`www/index.html`: no structural change (reuses `#action-buttons`).

CSS (`www/css/table.css` or `overlays.css`, alongside the existing button
classes): an "armed/lit" style for the toggle.

**No** `src/lib.rs` / WASM change; **no** rebuild required for this feature.

## Error handling & edge cases

- Human all-in / already folded / hand over → toggle not shown (or shown inert
  and never fires because `autoActFold` finds neither Check nor Fold legal).
- If `autoActFold` fires and `human_action` returns a recoverable error
  (unexpected, since Check/Fold are always in the offered `legal_actions` set),
  fall back to the existing error path in `onHumanAction` (re-render buttons,
  leave the human to act). The toggle is already disarmed at this point.
- Switching tabs to Arena while armed: no explicit reset is needed. `switchTab`
  sets `playLoopRunning = false` (halting the Play bots loop), Arena hides
  `#action-buttons` via CSS and never calls `renderActionButtons` (its only fire
  path), so an armed flag cannot fire in Arena. Returning to Play resumes the
  same hand, where the still-armed toggle behaving as armed is the intended
  behavior. The arm is cleared on the next new game (`beginNewGame`) and on hand
  completion (`HandComplete`), so it cannot leak into a *later* play hand.

  *(Implementation note: an earlier draft of this doc said the arm is reset "on
  tab switch"; the final code resets on new-game and hand-completion instead,
  which the whole-branch review confirmed is both safe and preferable — no
  Arena fire path exists, so a tab-switch reset would only discard a Play
  intent the user may still want on return.)*

## Testing

One Playwright spec (Play mode):

1. Start a hand and `window.__PK0__.setInstant()` to zero pacing (per the known
   game-speed gotcha — the Turbo slider alone still flakes).
2. During the bots-acting window, click `[data-act="prefold"]` to arm.
3. Assert the hero auto-acts on their turn: hero's action label shows Fold (or
   Check when unopposed) without a manual button press, and the toggle is
   disarmed afterward.

Because the deal is not seeded (entropy RNG), the test asserts the *mechanism*
(hero auto-acted / toggle disarmed), not a specific board or result.
