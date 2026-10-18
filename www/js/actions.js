// The action row, as data. Pure: a GameState (the JSON `get_state()` returns)
// in, the buttons to offer out — label, class, and what pressing one does.
// No DOM and no closure state, so main.js's dock and the embed's own row
// (EPIC-56) render the same choices from the same function.
//
// Lifted from main.js's renderActionButtons (EPIC-56 Phase 1). The pre-action
// Check/Fold check that preceded it stays in main.js: it is the dock's
// behaviour, not the row's.

/** @returns {Array<{label: string, cls: string, action: string, betAction?: string, amount?: number}>} */
export function buttonsFor(state) {
  const actions = state.legal_actions ?? [];
  const btns = [];

  for (const action of actions) {
    if (action === 'Fold') {
      btns.push({ label: 'Fold', cls: 'danger', action });
    } else if (action === 'Check') {
      btns.push({ label: 'Check', cls: 'safe', action });
    } else if (action === 'Call') {
      // pkcore always offers Call and converts a short stack into a partial
      // all-in call, so clamp the label to what the hero can actually put in.
      const stack = state.max_bet ?? 0;
      const amt = Math.min(state.to_call ?? 0, stack);
      const short = amt < (state.to_call ?? 0);
      btns.push({ label: 'Call $' + amt.toLocaleString() + (short ? ' (all-in)' : ''), cls: '', action });
    } else if (action === 'Bet' || action === 'Raise') {
      const minRaise = state.min_raise ?? 100;
      const maxBet   = state.max_bet   ?? 10000;
      const pot      = state.pot       ?? 0;
      // Min raise — always shown
      btns.push({ label: 'Min $' + minRaise.toLocaleString(), cls: 'primary', action: 'bet-direct', betAction: action, amount: minRaise });
      // ½ pot — shown when it's meaningfully above min and below max
      const halfPot = Math.round(pot / 2);
      if (halfPot > minRaise && halfPot < maxBet)
        btns.push({ label: '½ Pot $' + halfPot.toLocaleString(), cls: 'primary', action: 'bet-direct', betAction: action, amount: halfPot });
      // Full pot — shown when it's above min and below max (all-in handles the rest)
      if (pot > minRaise && pot < maxBet)
        btns.push({ label: 'Pot $' + pot.toLocaleString(), cls: 'primary', action: 'bet-direct', betAction: action, amount: pot });
      // Custom slider
      btns.push({ label: action + '…', cls: 'primary', action: 'bet-open', betAction: action });
    } else if (action === 'AllIn') {
      const amt = state.max_bet ?? 0;
      btns.push({ label: 'All-In $' + amt.toLocaleString(), cls: 'danger', action: 'allin', amount: amt });
    }
  }

  // Undo — step back to before your last action (within the current hand
  // only; the stack is cleared at each next_hand boundary). Surfaced by the
  // engine as `can_undo`.
  if (state.can_undo) {
    btns.push({ label: '↶ Undo', cls: '', action: 'undo' });
  }

  return btns;
}
