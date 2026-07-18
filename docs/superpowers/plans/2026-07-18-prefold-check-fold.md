# Prefold (pre-action Check/Fold) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an online-poker-style pre-action **Check/Fold** toggle button that appears in Play mode while the bots are acting, so the human can commit in advance to folding (or checking) on their next turn.

**Architecture:** Pure client-side, entirely in `www/js/main.js` plus a little CSS. A module-level `preFoldArmed` flag holds the armed state. A single toggle button renders into the existing `#action-buttons` area during the bots-acting loop. When the human's turn arrives and the flag is set, the existing `onHumanAction()` path is invoked automatically for `Check` or `Fold`, and the normal button render is skipped. No Rust/WASM change — every field needed (`legal_actions`, `phase`, `hero.state`) is already in the `GameState` JSON.

**Tech Stack:** Vanilla ES-module JS (`www/js/main.js`), plain CSS (`www/css/layout.css`), Playwright + TypeScript specs (`tests/`), Rust/WASM engine (unchanged).

## Global Constraints

- Play mode only; Arena mode has no human and no action buttons (`layout.css:122` hides `#action-buttons` in arena).
- **No** engine (`src/lib.rs`) or WASM rebuild.
- Ephemeral state — `preFoldArmed` is **never** written to `localStorage` (unlike the settings toggles).
- Check/Fold semantics: facing a bet (`legal_actions` includes `Fold`) → auto-**Fold**; can check for free (`legal_actions` includes `Check`) → auto-**Check**; neither → do nothing, render normal buttons.
- One-shot: the arm is consumed (cleared) the moment it fires; it also clears on a new game and on hand completion.
- The toggle button must carry `data-act="prefold"` (Playwright convention; see `renderButtons` at `main.js:793`).
- `state.hero.state` is one of `"Active"`, `"AllIn"`, `"Fold"`, `"Out"` (`state_to_str`, `lib.rs:2463`). The toggle shows only when it is `"Active"`.

---

### Task 1: Pre-action decision helper + armed state + test hook

Pure, dependency-free decision logic plus the module state var, exposed on the `window.__PK0__` debug hook so it can be unit-tested deterministically without driving a live hand.

**Files:**
- Modify: `www/js/main.js` — add module var near `pendingBetAction` (`main.js:739`); add `prefoldDecision()` helper; extend `window.__PK0__` (`main.js:298-314`).
- Test: `tests/prefold.spec.ts` (create)

**Interfaces:**
- Produces:
  - `let preFoldArmed` — module-scoped boolean, initial `false`.
  - `function prefoldDecision(legalActions)` → returns `'Fold'`, `'Check'`, or `null` (given an array of legal-action strings).
  - `window.__PK0__.prefoldDecision(legalActions)` → same, for tests.
  - `window.__PK0__.play.getPrefold()` → current `preFoldArmed`.
  - `window.__PK0__.play.setPrefold(v)` → sets `preFoldArmed = !!v` (test helper).

- [ ] **Step 1: Write the failing test**

Create `tests/prefold.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { waitForBoot } from './helpers';

test.describe('prefold decision logic', () => {
  test('folds when facing a bet, checks when free, else null', async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);

    const decide = (legal: string[]) =>
      page.evaluate((l) => (window as any).__PK0__.prefoldDecision(l), legal);

    // Facing a bet: Fold is offered → fold.
    expect(await decide(['Fold', 'Call', 'Raise', 'AllIn'])).toBe('Fold');
    // No bet facing hero: Check is offered, no Fold → check.
    expect(await decide(['Check', 'Bet', 'AllIn'])).toBe('Check');
    // Nothing to do (e.g. hero already all-in): null.
    expect(await decide([])).toBe(null);
    // Fold takes precedence over Check if both somehow present.
    expect(await decide(['Fold', 'Check'])).toBe('Fold');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx playwright test prefold.spec.ts -g "folds when facing" --project=chromium`
Expected: FAIL — `window.__PK0__.prefoldDecision is not a function`.

- [ ] **Step 3: Add the module var and helper**

In `www/js/main.js`, find the action-buttons section (`main.js:738-739`):

```js
    // ── Action buttons ────────────────────────────────────────────────────────
    let pendingBetAction = 'Bet';
```

Replace with:

```js
    // ── Action buttons ────────────────────────────────────────────────────────
    let pendingBetAction = 'Bet';

    // ── Prefold (pre-action Check/Fold) ───────────────────────────────────────
    // When armed during the bots-acting window, the hero auto check/folds the
    // moment it becomes their turn. Ephemeral: never persisted, cleared on fire,
    // on a new game, and on hand completion. See docs/superpowers/specs.
    let preFoldArmed = false;

    // Given the engine's legal-action list, decide the pre-action:
    //   facing a bet (Fold offered) → 'Fold'; can check for free → 'Check';
    //   neither actionable → null. Fold wins if both appear.
    function prefoldDecision(legalActions) {
      const legal = legalActions ?? [];
      if (legal.includes('Fold')) return 'Fold';
      if (legal.includes('Check')) return 'Check';
      return null;
    }
```

- [ ] **Step 4: Expose the test hooks on `window.__PK0__`**

In `www/js/main.js`, extend the hook object (`main.js:298-314`). Change the closing lines from:

```js
          setInstant: () => { BOT_ACTION_MS = 0; HAND_COMPLETE_MS = 0; },
        };
```

to:

```js
          setInstant: () => { BOT_ACTION_MS = 0; HAND_COMPLETE_MS = 0; },
          // Prefold test/debug hooks (see docs/superpowers/specs prefold design).
          prefoldDecision: (legal) => prefoldDecision(legal),
        };
        window.__PK0__.play.getPrefold = () => preFoldArmed;
        window.__PK0__.play.setPrefold = (v) => { preFoldArmed = !!v; };
```

- [ ] **Step 5: Run test to verify it passes**

Run: `npx playwright test prefold.spec.ts -g "folds when facing" --project=chromium`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add www/js/main.js tests/prefold.spec.ts
git commit -m "feat(prefold): pre-action decision helper + armed-state hooks"
```

---

### Task 2: Render the arm-able toggle during the bots-acting window

Draw the single Check/Fold toggle into `#action-buttons` while bots act (hero still `"Active"`), clicking it flips `preFoldArmed` and updates the lit state. Add the CSS for the armed look.

**Files:**
- Modify: `www/js/main.js` — add `renderPreAction()`; call it in `stepBotsUntilHuman()` (`main.js:197-198`).
- Modify: `www/css/layout.css` — armed/lit style near the action-button rules (`layout.css:104-118`).
- Test: `tests/prefold.spec.ts` (append)

**Interfaces:**
- Consumes: `preFoldArmed`, `prefoldDecision` (Task 1); `renderButtons` container `#action-buttons`.
- Produces: `function renderPreAction(state)` — renders/clears the toggle based on `state.hero.state === 'Active'`.

- [ ] **Step 1: Write the failing test**

Append to `tests/prefold.spec.ts`. This helper drives the live hand until the toggle appears (a 9-handed table almost always has a bots-before-hero window at hand 1 or right after one hero action), then verifies arm/disarm:

```ts
import { startGame, waitForHumanTurn } from './helpers';

// Drive the hand passively until the prefold toggle is visible. Returns true
// once found, false if it never appeared within `maxHeroActions` decisions.
async function reachPrefoldWindow(page, maxHeroActions = 12): Promise<boolean> {
  for (let i = 0; i < maxHeroActions; i++) {
    const found = await page
      .waitForSelector('[data-act="prefold"]', { timeout: 8000, state: 'visible' })
      .then(() => true)
      .catch(() => false);
    if (found) return true;
    // No window yet — if it's the hero's turn, pass passively and try the next street.
    const heroTurn = await page
      .waitForFunction(() => {
        const btns = document.querySelectorAll('#action-buttons button');
        return [...btns].some((b: any) => !b.disabled && b.id !== 'btn-new-game');
      }, { timeout: 8000 })
      .then(() => true)
      .catch(() => false);
    if (!heroTurn) return false;
    const check = page.locator('#action-buttons button[data-act="check"]');
    const call = page.locator('#action-buttons button[data-act="call"]');
    if (await check.count()) await check.first().click();
    else if (await call.count()) await call.first().click();
    else break; // only fold/raise available — passing would change the test's intent
  }
  return false;
}

test.describe('prefold toggle', () => {
  test('appears while bots act and arms/disarms on click', async ({ page }) => {
    await startGame(page);
    await reachPrefoldWindow(page); // may act the hero forward a street or two

    const toggle = page.locator('[data-act="prefold"]');
    await expect(toggle).toBeVisible();
    await expect(toggle).not.toHaveClass(/armed/);

    await toggle.click();
    await expect(toggle).toHaveClass(/armed/);
    expect(await page.evaluate(() => (window as any).__PK0__.play.getPrefold())).toBe(true);

    await toggle.click();
    await expect(toggle).not.toHaveClass(/armed/);
    expect(await page.evaluate(() => (window as any).__PK0__.play.getPrefold())).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx playwright test prefold.spec.ts -g "appears while bots act" --project=chromium`
Expected: FAIL — no `[data-act="prefold"]` element ever appears (timeout).

- [ ] **Step 3: Add `renderPreAction()`**

In `www/js/main.js`, immediately after the `prefoldDecision` function added in Task 1, add:

```js
    // Draw (or clear) the pre-action Check/Fold toggle into #action-buttons.
    // Only shown while the hero can still act this hand (state === 'Active').
    // Clicking flips preFoldArmed and re-renders the lit state.
    function renderPreAction(state) {
      const container = document.getElementById('action-buttons');
      while (container.firstChild) container.removeChild(container.firstChild);
      if (state?.hero?.state !== 'Active') return;

      const btn = document.createElement('button');
      btn.dataset.act = 'prefold';
      btn.className = 'prefold' + (preFoldArmed ? ' armed' : '');
      btn.textContent = (preFoldArmed ? '✓ ' : '') + 'Check/Fold';
      btn.title = 'Auto check/fold on your turn';
      btn.addEventListener('click', () => {
        preFoldArmed = !preFoldArmed;
        renderPreAction(state);
      });
      container.appendChild(btn);
    }
```

- [ ] **Step 4: Call it from the bots-acting loop**

In `stepBotsUntilHuman()` (`main.js:197-199`), change:

```js
        const state = JSON.parse(_playMod.get_state());
        renderTableVisuals(state);
        await new Promise(r => setTimeout(r, BOT_ACTION_MS));
```

to:

```js
        const state = JSON.parse(_playMod.get_state());
        renderTableVisuals(state);
        renderPreAction(state);   // show the pre-action Check/Fold toggle while bots act
        await new Promise(r => setTimeout(r, BOT_ACTION_MS));
```

- [ ] **Step 5: Add the CSS**

In `www/css/layout.css`, after the action-button focus rule (`layout.css:118`), add:

```css
#action-buttons button.prefold {
  border: var(--btn-min-border); background: var(--btn-min-bg); color: var(--btn-min-col);
  opacity: .8;
}
#action-buttons button.prefold.armed {
  border: var(--btn-fold-border); background: var(--btn-fold-bg); color: var(--btn-fold-col);
  opacity: 1; box-shadow: 0 0 0 2px var(--accent);
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `npx playwright test prefold.spec.ts -g "appears while bots act" --project=chromium`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add www/js/main.js www/css/layout.css tests/prefold.spec.ts
git commit -m "feat(prefold): render arm-able Check/Fold toggle during bots' turn"
```

---

### Task 3: Auto-fire on the hero's turn + resets

When the hero's turn arrives with the toggle armed, auto-fire Check/Fold and skip the normal button render. Clear the arm on fire, on a new game, and on hand completion.

**Files:**
- Modify: `www/js/main.js` — add `autoActFold()`; intercept at top of `renderActionButtons()` (`main.js:741`); reset in `beginNewGame()` (`main.js:68`) and the `HandComplete` branch (`main.js:649`).
- Test: `tests/prefold.spec.ts` (append)

**Interfaces:**
- Consumes: `preFoldArmed`, `prefoldDecision`, `renderPreAction` (Tasks 1–2); `onHumanAction(action, amount, state)` (`main.js:469`).
- Produces: `function autoActFold(state)` → fires the pre-action via `onHumanAction` and returns `true`, or returns `false` if there is nothing to auto-do.

- [ ] **Step 1: Write the failing test**

Append to `tests/prefold.spec.ts` (reuses `reachPrefoldWindow` from Task 2):

```ts
test.describe('prefold auto-fire', () => {
  test('armed toggle auto check/folds on the hero turn and disarms', async ({ page }) => {
    await startGame(page);
    const reached = await reachPrefoldWindow(page);
    expect(reached).toBe(true);

    await page.locator('[data-act="prefold"]').click();          // arm
    expect(await page.evaluate(() => (window as any).__PK0__.play.getPrefold())).toBe(true);

    // On the hero's next turn the pre-action fires automatically: the normal
    // action buttons (fold/check/call/raise) must NOT be left waiting for a click.
    // Either the hand advances (fold) or the hero checks and bots resume — in
    // both cases the arm is consumed.
    await page.waitForFunction(
      () => (window as any).__PK0__.play.getPrefold() === false,
      { timeout: 15_000 },
    );
    expect(await page.evaluate(() => (window as any).__PK0__.play.getPrefold())).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx playwright test prefold.spec.ts -g "auto check/folds" --project=chromium`
Expected: FAIL — `getPrefold()` stays `true` (nothing consumes the arm; the hero's real buttons render and wait).

- [ ] **Step 3: Add `autoActFold()`**

In `www/js/main.js`, immediately after `renderPreAction()` (added in Task 2), add:

```js
    // If the pre-action toggle is armed, auto-act on the hero's turn:
    // fold when facing a bet, else check. Returns true if it fired (the arm is
    // consumed one-shot before firing so it cannot recurse across streets).
    function autoActFold(state) {
      const action = prefoldDecision(state.legal_actions);
      if (!action) return false;   // hero can't check/fold (e.g. all-in) — let them decide
      preFoldArmed = false;
      onHumanAction(action, 0, state);
      return true;
    }
```

- [ ] **Step 4: Intercept at the top of `renderActionButtons()`**

In `www/js/main.js`, change the start of `renderActionButtons` (`main.js:741-742`) from:

```js
    function renderActionButtons(state) {
      const actions = state.legal_actions ?? [];
```

to:

```js
    function renderActionButtons(state) {
      // Pre-action: if the hero armed Check/Fold while bots acted, fire it now
      // instead of rendering buttons. onHumanAction() re-enters the bot loop.
      if (preFoldArmed && autoActFold(state)) return;
      const actions = state.legal_actions ?? [];
```

- [ ] **Step 5: Reset the arm on a new game**

In `beginNewGame()` (`main.js:68`), add `preFoldArmed = false;` as the first line of the function body. For example, if it reads:

```js
    function beginNewGame() {
      // ...existing body...
    }
```

make the first statement:

```js
    function beginNewGame() {
      preFoldArmed = false;   // never carry a pre-action arm into a new game
      // ...existing body...
    }
```

- [ ] **Step 6: Reset the arm on hand completion**

In the `HandComplete` branch of `renderState()` (`main.js:644-649`), change:

```js
      if (state.phase === 'HandComplete') {
        const street = state.street ?? '';
        appendHandLog('Hand #' + state.hand_number + ' complete — ' + street);
        enableYamlDownload();
        noteHandCompleted('play');
        renderButtons([]);
```

to:

```js
      if (state.phase === 'HandComplete') {
        preFoldArmed = false;   // a pre-action arm does not carry across hands
        const street = state.street ?? '';
        appendHandLog('Hand #' + state.hand_number + ' complete — ' + street);
        enableYamlDownload();
        noteHandCompleted('play');
        renderButtons([]);
```

- [ ] **Step 7: Run test to verify it passes**

Run: `npx playwright test prefold.spec.ts -g "auto check/folds" --project=chromium`
Expected: PASS.

- [ ] **Step 8: Run the full prefold spec + a quick regression pass**

Run: `npx playwright test prefold.spec.ts game.spec.ts --project=chromium`
Expected: all PASS (prefold behavior plus existing fold/check game flow unaffected).

- [ ] **Step 9: Commit**

```bash
git add www/js/main.js tests/prefold.spec.ts
git commit -m "feat(prefold): auto check/fold on hero turn + reset on new game/hand"
```

---

## Self-Review

**Spec coverage** (against `docs/superpowers/specs/2026-07-18-prefold-check-fold-design.md`):
- Pre-action timing (shows during bots-acting window) → Task 2 (`renderPreAction` in `stepBotsUntilHuman`).
- Single arm-able toggle, `data-act="prefold"`, lit when armed → Task 2 (render + CSS).
- Check/Fold semantics (fold vs check vs nothing) → Task 1 (`prefoldDecision`), fired by Task 3 (`autoActFold`).
- Shows only when hero can still act (`hero.state === 'Active'`) → Task 2 guard.
- One-shot disarm on fire → Task 3 (`autoActFold` clears before firing).
- Reset on new hand / new game; not persisted to localStorage → Task 3 (Steps 5–6); no `localStorage` code anywhere.
- No engine/WASM change → confirmed; all tasks touch only `main.js`, `layout.css`, `tests/`.
- Testing per the game-speed / entropy-deal gotchas → tests assert the *mechanism* (arm state, toggle class), never a specific board; the pure-decision test (Task 1) is fully deterministic.

**Placeholder scan:** none — every code step shows complete code and exact edits.

**Type consistency:** `prefoldDecision(legalActions)` returns `'Fold' | 'Check' | null`; `renderPreAction(state)` and `autoActFold(state)` both take the parsed `GameState`; `preFoldArmed` is the single shared boolean referenced identically across all three tasks; `data-act="prefold"` and the `.prefold`/`.armed` classes match between the renderer (Task 2) and both tests and CSS.
