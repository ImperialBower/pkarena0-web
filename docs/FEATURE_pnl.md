# Lifetime P&L

## Motivation

Players want a simple way to see how they're doing across all games — net winnings/losses since they started using the app. The score bar's existing **Chips** display only tells you about the current buy-in; once you click **New Game**, that delta disappears. Lifetime P&L persists in `localStorage` so it survives reloads, mobile-tab eviction, and quitting/reopening the app.

This replaces an earlier per-session P&L (removed in 0.1.x) that duplicated the chip count. The lifetime version is the one worth having.

## Behavior

The Play-tab score bar shows a P&L slot to the right of Chips:

```
Hand: 4 (50/100)   Chips: $10,500   P&L: +$300   ⚙
```

The displayed value is **cumulative net result across all completed Play-tab games**, plus the current game's in-flight delta.

| Color | Meaning |
|---|---|
| Green (`#44dd66`) | Positive lifetime |
| Red (`#ff6666`) | Negative lifetime |
| Default text | Exactly zero |

## Design decisions

| Decision | Choice |
|---|---|
| What counts toward lifetime | Play-tab games only. Arena (all-bot) does not contribute. |
| Commit trigger | On `SessionOver` (chips hit zero) **and** on **New Game** click from a non-bust state. |
| Display formula | `lifetime_committed + (current_chips − 10_000)`, single combined number, live-updating. After commit, the live-delta term is gated to zero to avoid double-counting. |
| Reset | Button in the Settings panel with `confirm()` dialog. |
| Score-bar placement | New slot after Chips, label `P&L`, sign-prefixed dollar amount. |
| Adjacent layout tweak | Compact `Hand` + `Blinds` into one slot: `Hand: 4 (50/100)` to free horizontal room. |

## Architecture

All changes live in `www/index.html`. Three concerns: **state**, **commit lifecycle**, **render**.

### State

```js
const STARTING_CHIPS = 10_000;
let lifetimePnl = parseInt(localStorage.getItem('lifetimePnl') ?? '0', 10) || 0;
let currentGameChips = STARTING_CHIPS;
let pendingGameCommitted = false;
```

Only `lifetimePnl` is persisted. `currentGameChips` is refreshed on every render so the commit functions don't have to round-trip through WASM. `pendingGameCommitted` makes `commit` idempotent — it's the lynchpin of the double-commit defense (see Edge cases).

### Commit lifecycle

```js
function commitCurrentGameToLifetime(chipsAtCommit = currentGameChips) {
  if (pendingGameCommitted) return;
  lifetimePnl += chipsAtCommit - STARTING_CHIPS;
  localStorage.setItem('lifetimePnl', String(lifetimePnl));
  pendingGameCommitted = true;
}

function beginNewGame() {
  commitCurrentGameToLifetime();
  pendingGameCommitted = false;
  currentGameChips = STARTING_CHIPS;
}
```

`commitCurrentGameToLifetime` is invoked from the `SessionOver` branch with the authoritative final chip count. `beginNewGame` runs at the top of every **New Game** code path (both the `startNewGame` function and the inline handler in the dynamic action-button setup).

### Render

```js
function renderPnlSlot(chipsForCalc = currentGameChips) {
  const liveDelta = pendingGameCommitted ? 0 : (chipsForCalc - STARTING_CHIPS);
  const total = lifetimePnl + liveDelta;
  const el = document.getElementById('sc-pnl');
  el.textContent = (total >= 0 ? '+' : '') + '$' + total.toLocaleString();
  el.className = total > 0 ? 'positive' : total < 0 ? 'negative' : '';
}
```

`pendingGameCommitted` gates the live-delta term. After `SessionOver` commits, the chip stack is still zero, so the displayed total must drop the live delta or it would double-count the bust.

`renderPnlSlot()` is called from `renderTableVisuals`, the `HandComplete` post-settlement block, the `SessionOver` branch of `renderState`, the reset handler, and at module init.

## Edge cases

- **Cold start, clean localStorage.** `getItem` returns `null`, falls back to `'0'`, parses to `0`. No localStorage write happens until the first commit.
- **Mid-game reload (mobile-tab eviction, Cmd-R).** `lifetimePnl` survives. The in-flight game's delta is lost — by design, since the game itself didn't end naturally.
- **Reset right after `SessionOver`, before clicking New Game.** `pendingGameCommitted` stays `true`. The next **New Game** is a no-op for commit. No phantom −$10,000.
- **Reset mid-game.** `pendingGameCommitted` is already `false` and stays `false`. The current game's delta is preserved in the live calculation and commits normally on its next end.
- **Tab switch to Arena.** The P&L slot hides via the existing `play-only` CSS class. The Arena adapter never touches `lifetimePnl`.

## Storage

| Key | Type | Format |
|---|---|---|
| `lifetimePnl` | `localStorage` | Decimal integer, signed, as a string. Example: `"-9500"` |

The value is the sum of every committed game's `final_chips − 10_000`. No history, no per-game breakdown — just the running total.

## Testing

- **Build:** `make build`. No Rust changes; this exercises the bundler and confirms the JS still parses.
- **Playwright:** `make test`. Existing tests don't reference `#sc-pnl`. Selector adjustments are only required if a test asserts on the literal `Blinds:` text in the score bar (that label is now removed in favor of the parenthesized form).
- **Manual smoke:**
  1. Cold-start in a private window. P&L reads `+$0`. No `lifetimePnl` key in localStorage.
  2. Play a winning game, click **New Game**. Verify localStorage updates and the displayed total stays consistent across the click.
  3. Reload mid-game. Lifetime survives; in-flight delta does not.
  4. Bust out to `SessionOver`. Commit happens once; clicking **New Game** afterward does not double-commit.
  5. Reset via Settings. Confirm dialog appears; **OK** zeroes; **Cancel** preserves.
  6. Switch to Arena. P&L slot is hidden. Arena gameplay does not affect lifetime.
