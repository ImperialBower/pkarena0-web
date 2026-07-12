# Showdown Reveal in the Hand Log — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** At a real showdown the hand log reveals every remaining player's hole cards + evaluated hand category, marks the winner(s) with the amount won, and stops mislabeling river folds as "Showdown".

**Architecture:** The Rust/WASM core (`src/lib.rs`) computes, before `end_hand()` resets the table, (a) a showdown-aware street label and (b) a one-shot per-player reveal list; both ride to the UI on `GameState`. The JS (`www/js/main.js`) renders the reveal block / uncontested line into the persistent `#hand-log`.

**Tech Stack:** Rust + `wasm-bindgen` (compiled via `make build` → `wasm-pack`), pkcore 0.2.1, vanilla JS, Playwright for integration tests.

## Global Constraints

- pkcore version is `0.2.1`; do not change the dependency. (`Cargo.toml`)
- The crate is `crate-type = ["cdylib"]` with no native Rust unit tests; the compile gate is `make build` and the behavioral gate is Playwright (`make test`). Follow that convention.
- `session.table` is a `pkcore::casino::table::Table` (already imported). `Table::effective_player_cards(seat) -> Option<Cards>` and `Table::seats.active_in_hand() -> Vec<u8>` are the engine methods used.
- `active_in_hand()` is `true` for all-in players and `false` for folded/out — all-ins are showdown participants.
- Card display format for the log: two-char codes like `"As"`, `"Ts"` (ten = `T`) produced by the existing `card_to_str(&Card) -> String`; the JS `cardsToLogStr(array)` converts them to `[A♠ 10♦]`.
- Hand-category strings come from the existing `hand_rank_name_to_str(HandRankName) -> Option<String>` ("Two Pair", "Full House", …).
- One-shot UI values follow the existing `LAST_HAND_RESULT` lifecycle: written in `next_hand()` under the `!had_audit_failure` gate, drained with `.take()` in `build_game_state()`.
- Global git rule: the human runs all state-changing git commands. Each "Commit" step below lists the exact command to hand to the user; do not run it yourself.

---

## File Structure

- `src/lib.rs` — Rust/WASM core. Modified: `street_from_board` signature + its two call sites; new `ShowdownPlayer` struct; new `LAST_SHOWDOWN` thread-local; `PreEnd` gains a `showdown` field populated in the pre-end snapshot; `next_hand()` commits it; `GameState` gains a `showdown` field drained in `build_game_state()`.
- `www/js/main.js` — UI. Modified: the `HandComplete` block (~567–612) appends the reveal block or the uncontested line.
- `tests/showdown-log.spec.ts` — new Playwright spec: seeded call-down run asserting the log records winners and shows a marked reveal.

---

### Task 1: Showdown-aware street label (Rust)

Make the "Showdown" label depend on a real showdown (2+ active) instead of board length alone, so a river fold reads "River".

**Files:**
- Modify: `src/lib.rs` — `street_from_board` (currently ~1289–1304) and its two call sites (~826 and ~1125).

**Interfaces:**
- Produces: `fn street_from_board(board_len: usize, is_showdown: bool) -> String` — later code and the JS `Hand #N complete — <street>` line rely on this returning `"Showdown"` only for a genuine showdown.

Note: native `cargo test` compiles and runs for this crate (verified), so this pure helper gets a real unit test.

- [ ] **Step 1: Write the failing unit test**

At the very bottom of `src/lib.rs` add a test module (it can call the private `street_from_board`):

```rust
#[cfg(test)]
mod street_tests {
    use super::street_from_board;

    #[test]
    fn full_board_is_showdown_only_when_contested() {
        // Genuine showdown: 5-card board, 2+ still in.
        assert_eq!(street_from_board(5, true), "Showdown");
        // River fold-out: 5-card board, only one left -> not a showdown.
        assert_eq!(street_from_board(5, false), "River");
    }

    #[test]
    fn pre_river_streets_ignore_showdown_flag() {
        assert_eq!(street_from_board(0, false), "Preflop");
        assert_eq!(street_from_board(3, false), "Flop");
        assert_eq!(street_from_board(4, false), "Turn");
        // Flag is irrelevant before the river.
        assert_eq!(street_from_board(3, true), "Flop");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test street_tests 2>&1 | tail -20`
Expected: FAIL — does not compile, because `street_from_board` still takes `SessionPhase`, not `bool` (a compile error is the "red" state here).

- [ ] **Step 3: Change `street_from_board` to take `is_showdown: bool`**

Replace the whole function (currently at ~`src/lib.rs:1289`):

```rust
fn street_from_board(board_len: usize, is_showdown: bool) -> String {
    match board_len {
        0 => "Preflop",
        3 => "Flop",
        4 => "Turn",
        // A full board is a showdown only when 2+ players revealed; a river
        // fold-out (or mid-hand) reads "River".
        _ => {
            if is_showdown {
                "Showdown"
            } else {
                "River"
            }
        }
    }
    .to_string()
}
```

- [ ] **Step 4: Fix the hand-history call site (`~src/lib.rs:826`)**

This call sits in the replay/step-building path with `SessionPhase::BotsActing`; it is never a showdown. Change:

```rust
    let street = street_from_board(table.board.len(), SessionPhase::BotsActing);
```

to:

```rust
    let street = street_from_board(table.board.len(), false);
```

- [ ] **Step 5: Fix the `build_game_state` call site (`~src/lib.rs:1125`)**

Replace:

```rust
        let street = street_from_board(table.board.len(), phase_val);
```

with:

```rust
        // A completed hand is a real showdown only when 2+ seats are still in
        // the hand (all-ins included); a river fold-out has exactly one.
        let is_showdown = phase_val == SessionPhase::HandComplete
            && table.seats.active_in_hand().len() >= 2;
        let street = street_from_board(table.board.len(), is_showdown);
```

- [ ] **Step 6: Run the unit test to verify it passes**

Run: `cargo test street_tests 2>&1 | tail -20`
Expected: PASS — `street_tests` both green.

- [ ] **Step 7: Build the WASM**

Run: `make build`
Expected: completes without error (no unused-import / signature errors). This is the compile gate for the WASM target.

- [ ] **Step 8: Commit**

Hand this to the user to run:

```bash
git add src/lib.rs && git commit -m "fix: label a hand 'Showdown' only when 2+ players reveal"
```

---

### Task 2: Showdown reveal data (Rust)

Capture, before `end_hand()` resets the table, each showdown participant's seat/name/hole-cards/hand-category, and surface it on `GameState`.

**Files:**
- Modify: `src/lib.rs` — imports (top, ~14–18); `PreEnd` struct (~259–267); the pre-end snapshot closure (~269–322); the `!had_audit_failure` block (~375–411); `LAST_SHOWDOWN` thread-local (~45–60); `GameState` struct (~951–974) + both construction sites (~1092–1112 and ~1177–1197).

**Interfaces:**
- Consumes: `street_from_board(_, bool)` from Task 1 (unrelated to this task's code, but same file).
- Produces: `GameState.showdown: Option<Vec<ShowdownPlayer>>` where
  `ShowdownPlayer { seat: u8, name: String, cards: Vec<String>, hand: String }`.
  JSON keys: `seat`, `name`, `cards` (array of two-char codes), `hand` (category string, may be empty). The JS in Task 3 relies on exactly these names.

- [ ] **Step 1: Add imports for the evaluator (top of `src/lib.rs`, near line 18)**

After `use pkcore::cards::Cards;` add:

```rust
use pkcore::arrays::seven::Seven;
use pkcore::analysis::eval::Eval;
```

- [ ] **Step 2: Add the `ShowdownPlayer` struct (near `PotResult`, ~`src/lib.rs:949`)**

```rust
/// One revealed hand at a genuine showdown (2+ players still in the hand).
/// Included in `GameState.showdown` immediately after such a hand ends.
#[derive(Serialize, Clone)]
struct ShowdownPlayer {
    seat: u8,
    name: String,
    cards: Vec<String>, // two-char codes, e.g. ["9s","As"]; JS renders [9♠ A♠]
    hand: String,       // evaluated category, e.g. "Two Pair" ("" if unknown)
}
```

- [ ] **Step 3: Add the `LAST_SHOWDOWN` thread-local (in the `thread_local!` block, after line 57)**

After the `LAST_HAND_RESULT` line add:

```rust
    /// One-shot showdown reveal populated by next_hand() (only when 2+ players
    /// reached showdown), consumed by build_game_state().
    static LAST_SHOWDOWN: RefCell<Option<Vec<ShowdownPlayer>>> = const { RefCell::new(None) };
```

- [ ] **Step 4: Add a `showdown` field to the `PreEnd` struct (~`src/lib.rs:259`)**

```rust
    struct PreEnd {
        hand_num: usize,
        button: u8,
        forced: ForcedBets,
        board_str: String,
        event_log: Vec<TableAction>,
        player_snapshot: Vec<(u8, String, usize, Option<String>)>,
        shuffled_deck_str: Option<String>,
        showdown: Option<Vec<ShowdownPlayer>>,
    }
```

- [ ] **Step 5: Populate `showdown` in the pre-end snapshot closure**

Inside the closure that builds `PreEnd` (the `SESSION.with(|s| { ... s.borrow().as_ref().map(|session| { ... })` block, ~269–322), before the `PreEnd { ... }` literal, compute the reveal. The `table` binding already exists in that scope:

```rust
            // Reveal every seat still in the hand (2+ = genuine showdown).
            // All-in players are included. Evaluate each seat's 7 cards for its
            // hand category. Skipped (None) when it was a fold-out.
            let active = table.seats.active_in_hand();
            let showdown: Option<Vec<ShowdownPlayer>> = if active.len() >= 2 {
                let players = active
                    .iter()
                    .filter_map(|&seat_num| {
                        let seat = table.seats.get_seat(seat_num)?;
                        let cards: Vec<String> = table
                            .dealt_hole_cards
                            .get(&seat_num)
                            .map(|bc| {
                                bc.as_slice()
                                    .iter()
                                    .filter(|c| **c != Card::BLANK)
                                    .map(card_to_str)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let hand = table
                            .effective_player_cards(seat_num)
                            .and_then(|c| Seven::try_from(c).ok())
                            .and_then(|seven| hand_rank_name_to_str(Eval::from(seven).hand_rank.name))
                            .unwrap_or_default();
                        Some(ShowdownPlayer {
                            seat: seat_num,
                            name: seat.player.handle.clone(),
                            cards,
                            hand,
                        })
                    })
                    .collect();
                Some(players)
            } else {
                None
            };
```

Then add `showdown,` to the `PreEnd { ... }` literal (alongside `shuffled_deck_str`).

- [ ] **Step 6: Commit the reveal to `LAST_SHOWDOWN` under the audit gate**

Inside `if !had_audit_failure { ... }` (~`src/lib.rs:375–411`), immediately after the existing `LAST_HAND_RESULT.with(|r| *r.borrow_mut() = Some(pot_results));` line, add:

```rust
                    LAST_SHOWDOWN.with(|r| *r.borrow_mut() = s.showdown.clone());
```

(`s` is the `PreEnd` snapshot in scope there. `.clone()` because `s` is borrowed immutably.)

- [ ] **Step 7: Add `showdown` to the `GameState` struct (~`src/lib.rs:973`)**

After the `last_result` field add:

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    showdown: Option<Vec<ShowdownPlayer>>,
```

- [ ] **Step 8: Set `showdown` in both `GameState` constructors**

In the early/no-session constructor (~`src/lib.rs:1111`), after `last_result: None,` add:

```rust
                showdown: None,
```

In the main constructor, after the existing `let last_result = LAST_HAND_RESULT.with(|r| r.borrow_mut().take());` (~1175) add:

```rust
        let showdown = LAST_SHOWDOWN.with(|r| r.borrow_mut().take());
```

and in the `GameState { ... }` literal (~1177–1197), after `last_result,` add:

```rust
            showdown,
```

- [ ] **Step 9: Build the WASM**

Run: `make build`
Expected: completes without error.

- [ ] **Step 10: Commit**

Hand this to the user to run:

```bash
git add src/lib.rs && git commit -m "feat: expose per-player showdown reveal on GameState"
```

---

### Task 3: Render the reveal in the hand log (JS) + Playwright acceptance

Render the winner-first marked reveal block at a real showdown, and a one-line uncontested result on a fold-out — both into the persistent `#hand-log`.

**Files:**
- Modify: `www/js/main.js` — the `if (state.phase === 'HandComplete')` block (~567–623), after `advanceHand()` returns `nextState`.
- Create: `tests/showdown-log.spec.ts`.

**Interfaces:**
- Consumes: `nextState.showdown` (`Array<{seat,name,cards,hand}>` or absent) and `nextState.last_result` (`Array<{seats,names,amount,hand}>`) from Task 2. Existing helpers `appendHandLog(str)` and `cardsToLogStr(arrayOfCodes)`.

- [ ] **Step 1: Write the failing Playwright acceptance test**

Create `tests/showdown-log.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { startGame, waitForHumanTurn, waitForBoot } from './helpers';

// Drive the hero as a pure call-down (never fold) so hands reach showdown,
// then assert the persistent hand log records winners and reveals hands.
test('hand log records winners and reveals showdown hands', async ({ page }) => {
  await startGame(page, 0.42);            // fixed Math.random -> deterministic
  await page.click('#log-toggle');        // open the (collapsed) hand-log aside

  // Play up to N hero decisions, always Check or Call, never Fold.
  for (let i = 0; i < 60; i++) {
    try {
      await waitForHumanTurn(page);
    } catch {
      break; // session over or no more turns
    }
    const check = page.locator('#action-buttons button:has-text("Check")');
    const call = page.locator('#action-buttons button:has-text("Call")');
    if (await check.count()) {
      await check.first().click();
    } else if (await call.count()) {
      await call.first().click();
    } else {
      break; // only Fold/Bet available in an odd spot; stop driving
    }
  }

  const log = (await page.locator('#hand-log').textContent()) ?? '';

  // The log must now record hand winners (old code never did).
  expect(log).toMatch(/wins \$[\d,]+/);
  // A genuine showdown must produce a winner-first marked reveal line.
  expect(log).toMatch(/★ .+: .+ — wins \$[\d,]+/);
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `make build && npx playwright test tests/showdown-log.spec.ts`
Expected: FAIL — the `★ … wins $` (and likely the `wins $`) assertion fails because the current log only prints `Hand #N complete — …`.

- [ ] **Step 3: Implement the reveal + uncontested rendering**

In `www/js/main.js`, inside the `HandComplete` block, after the existing `if (resultText) showHandResult(resultText, isWin);` line (~612), insert:

```js
        // ── Persist the outcome to the hand log (scrollback), not just banner ──
        const showdown = nextState.showdown;
        if (Array.isArray(showdown) && showdown.length) {
          // Winner seats + amount won per seat, summed across pots (side pots),
          // splitting an entry's amount across its seats for chopped pots.
          const wonBySeat = new Map();
          for (const pot of nextState.last_result ?? []) {
            const share = pot.amount / (pot.seats.length || 1);
            for (const s of pot.seats) wonBySeat.set(s, (wonBySeat.get(s) ?? 0) + share);
          }
          // Winners first, then the rest.
          const ordered = [...showdown].sort(
            (a, b) => (wonBySeat.has(b.seat) ? 1 : 0) - (wonBySeat.has(a.seat) ? 1 : 0),
          );
          for (const p of ordered) {
            const name = p.seat === 0 ? 'You' : p.name;
            const cards = cardsToLogStr(p.cards);
            const cat = p.hand ? p.hand : '';
            if (wonBySeat.has(p.seat)) {
              const amt = Math.round(wonBySeat.get(p.seat));
              appendHandLog(`★ ${name} ${cards}: ${cat} — wins $${amt.toLocaleString()}`);
            } else {
              appendHandLog(`  ${name} ${cards}: ${cat}`);
            }
          }
        } else if (result) {
          // Fold-out: one uncontested winner. No hand category (single-seat eval
          // is meaningless). The winner's own fold/action line is already above.
          const displayNames = result.names.map(n => (n === 'You' ? 'You' : n));
          const winnerStr = displayNames[0] ?? 'Unknown';
          appendHandLog(`${winnerStr} wins $${result.amount.toLocaleString()} uncontested`);
        }
```

(Note: `result` is the existing `const result = nextState.last_result?.[0];` from ~598, still in scope.)

- [ ] **Step 4: Run the acceptance test to verify it passes**

Run: `make build && npx playwright test tests/showdown-log.spec.ts`
Expected: PASS — both `wins $` and `★ … wins $` assertions match.

- [ ] **Step 5: Guard against regressions in the existing suite**

Run: `npx playwright test`
Expected: the pre-existing specs still pass (the new log lines are additive; no existing assertion depends on the old single completion line).

- [ ] **Step 6: Manual verification (`/run`)**

Serve the app (`make serve`), open it, and play/observe:
- a genuine multiway showdown → winner-first `★` reveal with categories;
- an all-in showdown with runout → all-in players revealed;
- a hand where everyone folds to one player on the river → `Hand #N complete — River` (not "Showdown") + `… wins $N uncontested`;
- a pre-river fold-out → correct street + uncontested line.

- [ ] **Step 7: Commit**

Hand this to the user to run:

```bash
git add www/js/main.js tests/showdown-log.spec.ts && git commit -m "feat: reveal showdown hands and winners in the hand log"
```

---

## Self-Review

**Spec coverage:**
- True-showdown reveal, winner-first + marked, categories for all → Task 2 (data) + Task 3 (render). ✓
- Split pots (per-winner share) → Task 3 Step 3 `wonBySeat` summing/splitting. ✓
- River-fold mislabel fix → Task 1. ✓
- Uncontested winner line in log → Task 3 Step 3 `else if (result)`. ✓
- All-ins counted as showdown participants → relies on `active_in_hand()` (Global Constraints); Task 2 Step 5. ✓
- Audit-failure path skips reveal → Task 2 Step 6 is inside the `!had_audit_failure` gate; `LAST_SHOWDOWN` stays `None`. ✓
- YAGNI non-goals (animations, folded cards, per-pot breakdown) → not implemented. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code. ✓

**Type consistency:** `ShowdownPlayer { seat, name, cards: Vec<String>, hand: String }` defined in Task 2 Step 2; JSON keys `seat/name/cards/hand` consumed verbatim in Task 3 Step 3. `GameState.showdown: Option<Vec<ShowdownPlayer>>` added in Task 2 Steps 7–8 and read as `nextState.showdown` in Task 3. `street_from_board(usize, bool)` defined in Task 1 Step 1 and called in Steps 2–3. ✓
