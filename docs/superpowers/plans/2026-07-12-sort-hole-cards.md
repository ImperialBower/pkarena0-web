# Sort Hole Cards High-to-Low — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display every player's hole cards highest-rank-first (poker convention) everywhere they appear — showdown reveal, hero dock, replay, bot action log, and hand-history YAML — by sorting once in the Rust/WASM core.

**Architecture:** Add one private helper `sorted_hand(&[Card]) -> Vec<Card>` to `src/lib.rs` that filters `Card::BLANK` and sorts descending via pkcore's derived `Card: Ord` (rank lives in the high bits of the Cactus-Kev `u32`, so descending is Ace→deuce). Route all five hole-card serialization sites through it, each keeping its own string formatter. No JS changes — only element order within the emitted arrays/strings changes.

**Tech Stack:** Rust + `wasm-bindgen` (compiled via `make build` → `wasm-pack`), pkcore 0.2.1, vanilla JS, Playwright for integration tests.

## Global Constraints

- pkcore version is `0.2.1`; do not change the dependency. (`Cargo.toml`)
- The crate is `crate-type = ["cdylib"]`; the compile gate is `make build` and the behavioral gate is Playwright (`npx playwright test`). Native `cargo test` also compiles and runs for this crate (used for pure-function unit tests).
- `Card` derives `Ord` (rank-primary via the `u32` bit layout); a descending sort is high-rank-first. Do not write a custom comparator.
- `card_to_str(&Card) -> String` produces two-char codes (`"As"`, `"Ts"` — ten = `T`, ASCII suit letters). `Card::from_str` accepts those same codes.
- Global git rule: the human runs all state-changing git commands. Each "Commit" step lists the exact command to hand to the user; do not run it yourself.

---

## File Structure

- `src/lib.rs` — Rust/WASM core. Add `sorted_hand` (near `card_to_str`, ~1396); update 5 call sites; add a `sort_tests` unit-test module at the bottom.
- No `www/js/main.js` changes.

---

### Task 1: The `sorted_hand` helper (Rust, unit-tested)

Add the pure sorting helper and prove its ordering + blank-dropping with a native unit test. No call sites change yet.

**Files:**
- Modify: `src/lib.rs` — add `sorted_hand` near `card_to_str` (~1396); add `sort_tests` module at the file bottom (after the existing `street_tests` module).

**Interfaces:**
- Produces: `fn sorted_hand(cards: &[Card]) -> Vec<Card>` — non-blank cards ordered high rank first. Task 2's five call sites consume it as `sorted_hand(src).iter().map(<formatter>)`.

- [ ] **Step 1: Write the failing unit test**

At the very bottom of `src/lib.rs`, after the existing `street_tests` module, add:

```rust
#[cfg(test)]
mod sort_tests {
    use super::{card_to_str, sorted_hand};
    use pkcore::card::Card;
    use std::str::FromStr;

    fn codes(cards: &[Card]) -> Vec<String> {
        sorted_hand(cards).iter().map(card_to_str).collect()
    }

    #[test]
    fn orders_high_rank_first_and_drops_blank() {
        let hand = [
            Card::from_str("2c").unwrap(),
            Card::from_str("As").unwrap(),
            Card::BLANK, // padding must be dropped
            Card::from_str("Td").unwrap(),
            Card::from_str("Kh").unwrap(),
        ];
        assert_eq!(codes(&hand), vec!["As", "Kh", "Td", "2c"]);
    }

    #[test]
    fn already_sorted_stays_sorted() {
        let hand = [
            Card::from_str("Ah").unwrap(),
            Card::from_str("Kd").unwrap(),
        ];
        assert_eq!(codes(&hand), vec!["Ah", "Kd"]);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test sort_tests 2>&1 | tail -20`
Expected: FAIL — does not compile, because `sorted_hand` does not exist yet (a compile error is the "red" state here).

- [ ] **Step 3: Add the `sorted_hand` helper**

Immediately above `fn card_to_str` (~`src/lib.rs:1396`) add:

```rust
/// Non-blank cards ordered high rank first (poker display convention).
/// Relies on pkcore's derived `Card: Ord` (rank-primary in the Cactus-Kev
/// u32); a descending sort is Ace-high first. Suit is a minor tiebreak.
fn sorted_hand(cards: &[Card]) -> Vec<Card> {
    let mut v: Vec<Card> = cards.iter().copied().filter(|c| *c != Card::BLANK).collect();
    v.sort_unstable_by(|a, b| b.cmp(a)); // descending: Ace-high first
    v
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test sort_tests 2>&1 | tail -20`
Expected: PASS — both `sort_tests` green.

- [ ] **Step 5: Build the WASM (guard the cdylib target)**

Run: `make build`
Expected: completes without error (the new fn is `dead_code` until Task 2 wires it — `sort_tests` uses it under `cfg(test)`, but `make build` does not compile tests, so expect an `unused` warning here; it disappears in Task 2. A warning is acceptable; an error is not).

- [ ] **Step 6: Commit**

Hand this to the user to run:

```bash
git add src/lib.rs && git commit -m "feat: add sorted_hand helper for high-to-low hole cards"
```

---

### Task 2: Route all five hole-card sites through `sorted_hand`

Wire the helper into every serialization site so all displays and the YAML emit sorted cards. Each site keeps its existing formatter; only order changes.

**Files:**
- Modify: `src/lib.rs` — five call sites (showdown reveal ~322, hand-history `hole_str` ~290, `ReplaySeat` ~894, `step_bot` result ~1093, `PlayerView` `show_cards` ~1282).

**Interfaces:**
- Consumes: `sorted_hand(&[Card]) -> Vec<Card>` from Task 1.

- [ ] **Step 1: Site 1 — showdown reveal (`ShowdownPlayer.cards`, ~322)**

Replace:

```rust
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
```

with:

```rust
                        let cards: Vec<String> = table
                            .dealt_hole_cards
                            .get(&seat_num)
                            .map(|bc| sorted_hand(bc.as_slice()).iter().map(card_to_str).collect())
                            .unwrap_or_default();
```

- [ ] **Step 2: Site 2 — hand-history `hole_str` YAML (~290)**

This site keeps its `c.to_string()` formatter and space-join — only order changes. Replace:

```rust
                    let hole_str = table
                        .dealt_hole_cards
                        .get(&seat_num)
                        .and_then(|bc| {
                            let s: String = bc
                                .as_slice()
                                .iter()
                                .filter(|c| **c != Card::BLANK)
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join(" ");
                            if s.is_empty() { None } else { Some(s) }
                        });
```

with:

```rust
                    let hole_str = table
                        .dealt_hole_cards
                        .get(&seat_num)
                        .and_then(|bc| {
                            let s: String = sorted_hand(bc.as_slice())
                                .iter()
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join(" ");
                            if s.is_empty() { None } else { Some(s) }
                        });
```

- [ ] **Step 3: Site 3 — `ReplaySeat.hole_cards` (~894)**

Replace:

```rust
            let cards: Vec<String> = s
                .cards
                .as_slice()
                .iter()
                .filter(|c| **c != Card::BLANK)
                .map(card_to_str)
                .collect();
```

with:

```rust
            let cards: Vec<String> = sorted_hand(s.cards.as_slice())
                .iter()
                .map(card_to_str)
                .collect();
```

- [ ] **Step 4: Site 4 — `step_bot()` result `hole_cards` (~1093)**

Replace:

```rust
                            let hole_cards: Vec<String> = session.table.seats.get_seat(seat)
                                .map_or_else(Vec::new, |s| {
                                    s.cards.as_slice().iter()
                                        .filter(|c| **c != Card::BLANK)
                                        .map(card_to_str)
                                        .collect()
                                });
```

with:

```rust
                            let hole_cards: Vec<String> = session.table.seats.get_seat(seat)
                                .map_or_else(Vec::new, |s| {
                                    sorted_hand(s.cards.as_slice()).iter().map(card_to_str).collect()
                                });
```

- [ ] **Step 5: Site 5 — `PlayerView.hole_cards`, `show_cards` branch (~1282)**

Replace:

```rust
        let cards: Vec<String> = s
            .cards
            .as_slice()
            .iter()
            .filter(|c| **c != Card::BLANK)
            .map(card_to_str)
            .collect();
```

with:

```rust
        let cards: Vec<String> = sorted_hand(s.cards.as_slice())
            .iter()
            .map(card_to_str)
            .collect();
```

Note: the sibling `else` branch that emits `"__"` blanks for face-down bots (~1291) is intentionally left untouched — it renders a count, not identities.

- [ ] **Step 6: Build the WASM**

Run: `make build`
Expected: completes without error and **without** the `unused` warning from Task 1 (all five sites now use `sorted_hand`).

- [ ] **Step 7: Re-run the unit tests**

Run: `cargo test sort_tests street_tests 2>&1 | tail -20`
Expected: PASS — all four tests green (nothing in Task 2 changes the helper).

- [ ] **Step 8: Guard against regressions in the Playwright suite**

Run: `npx playwright test`
Expected: all specs pass. The `showdown-log.spec.ts` regexes match card *content* (`★ .+: .+ — wins \$…`) regardless of order, so reordering keeps them green.

- [ ] **Step 9: Manual verification (`/run`)**

Serve the app (`make serve`), open it, open the hand log, set speed to Turbo, and play a call-down. Confirm high-to-low order in:
- the showdown reveal (`★ You [A♠ K♥]: …`);
- the hero dock (large hole cards);
- the bot action-log lines (`maniac [K♣ 9♦]: …`);
- the downloaded hand-history YAML (`hole_cards` per seat).

- [ ] **Step 10: Commit**

Hand this to the user to run:

```bash
git add src/lib.rs && git commit -m "feat: sort hole cards high-to-low across all displays"
```

---

## Self-Review

**Spec coverage:**
- Sort once in the Rust core → Task 1 helper. ✓
- All five sites (showdown reveal, hero dock/`PlayerView`, replay, bot log, YAML) → Task 2 Steps 1–5. ✓
- YAML keeps code format, changes only order → Task 2 Step 2 preserves `to_string()`. ✓
- Descending / Ace-high, no custom comparator → Task 1 Step 3 `b.cmp(a)`. ✓
- BLANK dropped → Task 1 helper `filter`; asserted in Task 1 Step 1 test. ✓
- No JS changes → Task 2 touches only `src/lib.rs`. ✓
- Face-down `"__"` branch untouched → Task 2 Step 5 note. ✓
- YAGNI (no toggle, no suit grouping, no replay-input change) → not implemented. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full before/after code. ✓

**Type consistency:** `sorted_hand(&[Card]) -> Vec<Card>` defined in Task 1 Step 3, consumed identically in all five Task 2 edits (`sorted_hand(src).iter().map(...)`). Test helpers `card_to_str`, `Card::from_str`, `Card::BLANK` verified to exist in pkcore 0.2.1. ✓
