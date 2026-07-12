# Sort Hole Cards High-to-Low — Design

**Goal:** Every place a player's hole cards are shown, display them in poker
convention — highest rank first (e.g. `[A♠ K♥]`, `[10♣ 3♠]`) — instead of the
current dealt order. Sort once, in the Rust/WASM core, so every render path
(showdown reveal, hero dock, replay, bot action log, and the hand-history YAML)
is consistent and cannot drift.

## Background

Hole cards are serialized to two-char code arrays/strings in `src/lib.rs` and
rendered by `www/js/main.js`. Today each site emits cards in dealt order via the
repeated pattern:

```rust
let cards: Vec<String> = <source>
    .as_slice()
    .iter()
    .filter(|c| **c != Card::BLANK)
    .map(card_to_str)
    .collect();
```

pkcore 0.2.1's `Card` derives `Ord` (`src/card.rs:29`). A `Card` is a Cactus-Kev
`u32` with the rank bit-flag in the high bits (`RANK_FLAG_FILTER = 0x1FFF_0000`,
bits 16–28) above the suit flags (`SUIT_FLAG_FILTER = 0xF000`, bits 12–15). So
the derived ordering is **rank-primary ascending, suit secondary**; a descending
sort yields Ace→deuce with suit as a minor tiebreak. No custom comparator needed.

## Design

### The helper

Add one private function to `src/lib.rs`:

```rust
/// Non-blank cards ordered high rank first (poker display convention).
fn sorted_hand(cards: &[Card]) -> Vec<Card> {
    let mut v: Vec<Card> = cards.iter().copied().filter(|c| *c != Card::BLANK).collect();
    v.sort_unstable_by(|a, b| b.cmp(a)); // descending: Ace-high first
    v
}
```

- Takes `&[Card]` — matches every current source, which exposes `.as_slice()`.
- Returns `Vec<Card>` (not codes) so each call site keeps its own formatter.
  `Card` is `Copy`, so `.copied()` is cheap.
- Folds the existing `filter(BLANK)` into one place.

**Why sort `Card`s, not the string codes:** lexical sorting of two-char codes
misorders (`"As"` < `"Kh"` as text, but A > K in poker). Sorting the typed
`Card` uses the real rank ordering.

### Call sites (5)

Each becomes `sorted_hand(<source>).iter().map(<existing formatter>)…`:

| # | Site (approx.) | Struct/field | Formatter (unchanged) |
|---|----------------|--------------|-----------------------|
| 1 | `ShowdownPlayer.cards` build (~323) | showdown reveal | `card_to_str` |
| 2 | `PlayerView.hole_cards`, `show_cards` (~1282) | hero dock / readouts | `card_to_str` |
| 3 | `ReplaySeat.hole_cards` (~894) | replay render | `card_to_str` |
| 4 | `step_bot()` result `hole_cards` (~1093) | bot action log | `card_to_str` |
| 5 | hand-history `hole_str` (~294) | YAML | `c.to_string()` + `.join(" ")` |

Site 5 keeps its `to_string()` formatter and space-join — **only the order
changes**, never the code format. The face-down branch (`"__"` blanks, ~1291)
is untouched: it renders count, not identities.

### Data flow

No interface changes. The JSON/YAML *shape* crossing the WASM→JS boundary is
identical (same arrays, same keys); only element order within each hole-card
array changes. `www/js/main.js` needs no edits — `cardsToLogStr` and the hero
dock render whatever order the core emits.

### Non-goals (YAGNI)

- No config toggle or per-user sort preference.
- No suit-based grouping or bridge-order suits — the `u32` suit tiebreak
  (clubs > diamonds > hearts > spades) is accepted as-is; suit order within a
  pair is cosmetic.
- No change to `inject_hole_cards` / replay *input* — a hand is a set, so
  sorting the *output* is order-independent and cannot affect round-trip.

## Testing

- **Unit (native `cargo test`, no WASM):** `sorted_hand` on a mixed, unsorted
  hand with a `BLANK` returns rank-descending codes and drops the blank. Card is
  `Ord`, so this compiles and runs under `cargo test` directly.
- **Compile gate:** `make build` (WASM target).
- **Regression gate:** the existing Playwright suite (`npx playwright test`) —
  the additive reorder must not break any assertion. The showdown-log spec's
  regexes match card content regardless of order, so they stay green.
- **Manual eyeball:** drive a call-down at Turbo speed and confirm reveal /
  dock / bot log / YAML all read high-to-low.

## Files

- `src/lib.rs` — add `sorted_hand`; update 5 call sites; add a unit test.
- No `www/js/main.js` changes.
