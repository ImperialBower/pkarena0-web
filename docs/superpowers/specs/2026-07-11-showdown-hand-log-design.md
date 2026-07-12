# Showdown reveal in the hand log — design

**Date:** 2026-07-11
**Status:** Approved (design), pending implementation plan

## Problem

At the end of a hand the persistent hand log only records:

```
Hand #1 complete — Showdown
```

It never shows which hands were revealed, who won, or the winning hand.
Worse, the "Showdown" label is board-length-driven, so a hand that reaches
a 5-card board but ends on a **fold** (only one player left) is still
labeled "Showdown" even though no cards were revealed.

Separately, the persistent log never records *who won* at all — the winner
is only shown in the transient banner (`showHandResult`) and status bar
(`setStatus`), never appended to the scrollback log.

## Goal

1. At a **true showdown** (2+ players still in the hand, including all-ins),
   reveal every remaining player's hole cards and evaluated hand category,
   mark the winner(s), and show the amount won — all in the persistent log.
2. Stop mislabeling river folds as "Showdown".
3. Record the winning outcome in the persistent log for both paths.

## Behavior

### True showdown (`active_in_hand().len() >= 2`)

Winner(s) first, marked with `★` and amount won; remaining revealed hands
beneath, indented. Split pots produce one `★` line per winner with that
winner's share.

```
Hand #3 complete — Showdown
★ gto [A♦ K♥]: Two Pair — wins $1,800
  You [9♠ A♠]: Ace High
```

### River fold (mislabel fix, `active_in_hand().len() == 1` on a 5-card board)

The completion label uses the real street (never "Showdown"), and a new
outcome line is appended:

```
Hand #1 complete — River
gto wins $2,400 uncontested
```

The "You fold" action is already the line immediately above (the hero's
final action), so it is not repeated. The uncontested winner line is the
new information.

Pre-river fold-outs (Preflop/Flop/Turn) were already labeled correctly;
they additionally gain the `… wins $N uncontested` outcome line.

## Data flow

Two engine facts drive the feature. Both are read **before**
`session.end_hand()` resets the table — i.e. in the pre-end snapshot block
of `next_hand()` (`src/lib.rs:270–322`), where the board is still full and
seats still hold their cards:

- `table.seats.active_in_hand().len()` distinguishes a fold-out (`1`) from
  a real showdown (`2+`). This mirrors pkcore's own `Showdown::process`,
  which branches on the same value. `active_in_hand()` uses
  `PlayerState::is_active()`, which is **true for `AllIn`** and false for
  `Fold`/`Out`/`Ready` — so all-in players are correctly included as
  showdown participants.
- For each active seat, the evaluated hand category comes from
  `table.effective_player_cards(seat)` → `Seven::try_from(cards)` →
  `Eval::from(seven)` → `hand_rank_name_to_str(eval.hand_rank.name)`
  (the same helper that already produces "Two Pair", "Full House", etc.
  for the winner summary).

### Rust changes (`src/lib.rs`)

1. New serializable struct:
   ```rust
   #[derive(Serialize)]
   struct ShowdownPlayer {
       seat: u8,
       name: String,
       cards: String, // raw hole cards, e.g. "9s As" (JS formats to [9♠ A♠])
       hand: String,  // evaluated category, e.g. "Two Pair"
   }
   ```
2. New one-shot `thread_local! LAST_SHOWDOWN: RefCell<Option<Vec<ShowdownPlayer>>>`,
   mirroring the existing `LAST_HAND_RESULT` lifecycle: written in
   `next_hand()` under the existing `!had_audit_failure` gate, drained
   (`take()`) in `build_game_state()`.
3. In the pre-end snapshot block, when `active_in_hand().len() >= 2`, build
   the `Vec<ShowdownPlayer>` for the active seats using the hole cards
   already gathered there plus the per-seat eval. Carry it on the `PreEnd`
   snapshot struct, then commit it to `LAST_SHOWDOWN` alongside
   `LAST_HAND_RESULT` inside the `!had_audit_failure` block (~`src/lib.rs:410`).
   When the chip audit fails, both are skipped (unchanged behavior).
4. Add `showdown: Option<Vec<ShowdownPlayer>>` to `GameState`; it rides on
   the same `nextState` as `last_result`.
5. Make the street label showdown-aware. `street_from_board` currently
   emits "Showdown" for any 5-card board at `HandComplete`. Change the call
   site in `build_game_state` (which has table access) so "Showdown" is
   emitted only when `active_in_hand().len() >= 2`; a 5-card-board fold-out
   resolves to "River". Board lengths < 5 are unaffected.

### JS changes (`www/js/main.js`, the `HandComplete` block ~567–612)

- The existing `Hand #N complete — <street>` line (line 569) and the
  banner/status behavior are unchanged. `state.street` is now correct
  because the Rust label fix runs at the `HandComplete` state build.
- After `advanceHand()` returns `nextState`:
  - If `nextState.showdown` is present and non-empty, append the
    winner-first `★` reveal block:
    - Winner seats = union of `nextState.last_result[*].seats`.
    - Amount won per winner = sum of `last_result[*].amount` across pots
      where that seat appears (handles main + side pots).
    - Winners sorted first; seat `0` renders as "You"; cards rendered via
      the existing `cardsToLogStr`.
    - Winner line: `★ ${name} ${cards}: ${hand} — wins $${amount}`.
    - Non-winner line: `  ${name} ${cards}: ${hand}`.
  - Else, if `last_result` has a winner (fold-out), append a single
    `${winnerStr} wins $${amount} uncontested` line (no hand category —
    a single-seat eval is meaningless).

## Testing

- **Rust unit tests:** street-label logic (5-card board + 1 active → "River";
  + 2 active → "Showdown"); `LAST_SHOWDOWN` populates for a scripted
  2-player showdown and stays `None` for a fold-out.
- **Build:** `make build` compiles the WASM without error.
- **Manual (`/run`):** play to (a) a genuine multiway showdown, (b) an
  all-in showdown with runout, (c) a river fold, (d) a split pot; verify
  each log block reads correctly.

## Scope / non-goals (YAGNI)

Not doing: reveal animations or reordering effects, showing folded players'
cards, board-texture annotations, or a per-pot side-pot breakdown in the log
(winner amount is summed across pots). These can be added later if wanted.
