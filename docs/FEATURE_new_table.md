# New Table

## Motivation

Players sometimes want a fresh start — different bots, a clean stack — without having to bust out first or reload the page. Maybe the current lineup is grinding them down. Maybe they're up money and want to lock in profit before variance steals it back. Either way, the only existing escape routes were (a) lose all your chips so `SessionOver` triggers the **New Game** button, or (b) Cmd-R.

This feature adds a **New Table** button to the score bar. Click it between hands (or any time you're not contesting a pot) to walk away: your current chips bank into lifetime P&L, and you sit down with $10,000 against a freshly shuffled lineup.

Lifetime P&L survives the transition — it always has, since it lives in `localStorage` (see [FEATURE_pnl.md](FEATURE_pnl.md)). All this feature does is reuse the same commit machinery already wired into **New Game** and `SessionOver`.

## Behavior

A new button appears in the score bar, after the P&L slot:

```
Hand: 4 (50/100)   Chips: $10,500   P&L: +$300   [ New Table ]   ⚙
```

The button is **disabled (greyed out) whenever the player is actively in the current hand** — meaning they're contesting a pot they haven't folded out of yet. Concretely, disabled whenever `hero.state` is one of: `YetToAct`, `Check`, `Call`, `Blind`, `Bet`, `Raise`, `ReRaise`, `AllIn`, or `Showdown` mid-hand.

It becomes **enabled** the moment the player is no longer contesting a hand: after they fold, between hands (`HandComplete`), at session end (`SessionOver`), or before the first hand has been dealt.

| `hero.state` / `phase` | Button |
|---|---|
| `phase = WaitingForHuman` (player's turn) | Disabled |
| `phase = BotsActing`, `hero.state = Call`/`Bet`/`AllIn`/etc. | Disabled |
| `phase = BotsActing`, `hero.state = Fold` (folded earlier) | Enabled |
| `phase = HandComplete` (cards revealed, results shown) | Enabled |
| `phase = SessionOver` | Enabled |
| `phase = Uninitialized` (cold start) | Enabled |

## Design decisions

| Decision | Choice |
|---|---|
| Disabled rule | Mirrors the Rust `is_in_hand()` helper at `src/lib.rs:860`, inverted. Single source of truth for "is the player committed to this pot." |
| Placement | Score bar, between P&L and the settings cog. Persistent, so its disabled/enabled state telegraphs whether the player can leave. |
| Confirmation | Conditional — only when `hero.chips > 10_000` (player is up money on this table). Uses `confirm("Walk away with $X profit? Your chips will be banked to lifetime P&L.")`. Losing players walk freely; winning players get a guard against fat-finger clicks that would prematurely lock in profit. |
| Arena mode | Button hidden via existing `play-only` CSS class. Arena (all-bot) never had per-table identity worth walking away from. |
| Reuse of existing flow | The click handler is a near-mirror of the inline `'new-game'` action button handler. Both call `beginNewGame()` (which commits chips to lifetime P&L) then `_playMod.init_game(seed)` (which fully replaces the WASM `SESSION` thread-local with a freshly shuffled 9-seat table). |

## Architecture

All changes live in `www/index.html`. No Rust/WASM changes are needed because `init_game()` already accepts a fresh seed and rebuilds the entire session from scratch.

### Markup

```html
<span class="play-only">P&amp;L: <strong id="sc-pnl">+$0</strong></span>
<button id="new-table-btn" class="play-only" title="Walk away to a new table with new bots" disabled>New Table</button>
```

The `play-only` class hides the button in Arena mode. The button starts `disabled` so it can't be clicked before the first state render.

### Enable/disable rule

```js
// States that mean the player is NOT actively contesting the current hand.
// Mirrors the Rust `is_in_hand()` helper (src/lib.rs:860) inverted.
const HERO_NOT_IN_HAND_STATES = new Set(['Fold', 'Ready', 'Out']);

function updateNewTableButton(state) {
  const btn = document.getElementById('new-table-btn');
  if (!btn) return;
  const phase = state?.phase;
  const heroState = state?.hero?.state;

  // Primary rule: if hero is actively in a hand, button is DISABLED.
  const heroNotInHand = !heroState || HERO_NOT_IN_HAND_STATES.has(heroState);

  // Phase override: HandComplete means the hand has fully resolved
  // (cards revealed, pots paid). Safe to walk away even if hero.state
  // is still 'Showdown' from the just-finished hand.
  // Uninitialized/SessionOver: no live hand exists.
  const phaseAllows = phase === 'Uninitialized'
                   || phase === 'HandComplete'
                   || phase === 'SessionOver';

  btn.disabled = !(heroNotInHand || phaseAllows);
}
```

`updateNewTableButton(state)` is called everywhere `renderPnlSlot()` is called — same render path, same triggers. When no state object is in scope (e.g. `clearTable()`), it's called with `null`, which keeps the button disabled.

### Click handler

```js
document.getElementById('new-table-btn').addEventListener('click', () => {
  const chips = currentGameChips;
  if (chips > STARTING_CHIPS) {
    const profit = chips - STARTING_CHIPS;
    if (!confirm('Walk away with $' + profit.toLocaleString() + ' profit? Your chips will be banked to lifetime P&L.')) return;
  }
  beginNewGame();
  hideBetControls();
  clearAllActions();
  hideHandResult();
  playBlindLevel = 0;
  gameSeed = Math.random();
  const s = JSON.parse(_playMod.init_game(gameSeed));
  updateUrlState('play', 1);
  renderTableVisuals(s);
  setStatus('New table — fresh bots dealing in…');
  enableCurrentGameButton();
  stepBotsUntilHuman();
});
```

This is intentionally a near-clone of the existing inline `'new-game'` handler. The only behavioral differences: the conditional confirm guard at the top, and a different status message ("New table — fresh bots dealing in…" vs "Dealing…").

## Edge cases

- **Click during the 2.2s `HandComplete` pause.** The hand has resolved, the auto-advance to the next hand hasn't fired yet, and the button is enabled. Clicking pre-empts the auto-advance: the `init_game()` call replaces the entire `SESSION` thread-local, so the queued `advanceHand()` becomes a no-op (its phase check finds `BotsActing` from the new game, not `HandComplete`).
- **Click after busting (`SessionOver`).** Both the **New Game** button (in the action row) and the **New Table** button are visible and enabled. Either works. `pendingGameCommitted` is already `true` from the bust commit, so `beginNewGame()`'s call to `commitCurrentGameToLifetime()` is a no-op — no phantom −$10,000.
- **Click after folding while bots play out the hand.** Hero's state is `Fold` so the button is enabled. Clicking abandons the hand mid-stream — bots' remaining actions never execute because their thread-local session gets replaced. The current chip count (which doesn't include any of the discarded pot) is what banks to lifetime P&L. This is the right semantic: you folded, you weren't going to win the pot anyway.
- **Click while up money.** Confirm dialog blocks the action. Cancel → no state change, including no commit. OK → commits and resets.
- **Rapid double-click.** First click triggers `init_game()` which immediately re-renders state with the new game in `BotsActing`, which calls `updateNewTableButton(s)` and disables the button (hero.state = YetToAct or similar). The second click is a no-op against a disabled button.
- **Arena mode.** Button hidden via `play-only` CSS. No JS guard needed.

## Testing

- **Build:** `make build`. No Rust changes; exercises the bundler and confirms the JS still parses.
- **Manual smoke:**
  1. Cold-start. Button is **enabled** (phase = Uninitialized). Click it — no confirm, fresh hand deals, lifetime P&L unchanged.
  2. Wait for the human's turn. Button is **disabled**. Try clicking — no-op.
  3. Call a bet, watch bots act on the flop/turn. Button stays **disabled** the whole time hero is contesting.
  4. Fold preflop. Bots play out the hand. Button is **enabled** during the bot loop.
  5. Reach `HandComplete`. Button is **enabled** during the 2.2s window.
  6. Win a hand to push hero chips above $10,000. Click **New Table** → confirm dialog with profit amount. Cancel → no change. Re-click and accept → P&L delta banks correctly.
  7. Bust out to `SessionOver`. Both **New Game** and **New Table** are available. Click **New Table** — should not double-commit (lifetime stays at the post-bust value).
  8. Switch to Arena tab. Button hidden.
