# EPIC-51: Player-Mode Table Setup — Opponent Count & Bot Selection

> **One-line:** Let the human choose *how many* bots they face (1–8) and
> *which archetypes* sit down — including the same archetype more than once
> — via a table-setup panel on the Play tab, replacing the hardcoded
> "always 8, always random" lineup while keeping today's behavior as the
> seeded-identical default.

## Status

| Component | Status |
|---|---|
| `TableConfig` + archetype registry (name → `BotProfile`) in `src/lib.rs` | Planned |
| `init_game_configured(seed, config_json)` WASM export; `init_game` = default wrapper | Planned |
| Duplicate archetypes: handle suffixing ("maniac 2") + archetype-keyed display | Planned |
| Seeded default-equivalence guarantee (old `init_game` behavior byte-identical) | Planned |
| Table-setup panel on Play tab (opponent count + bot picker) | Planned |
| `pkarena.tableConfig` localStorage persistence | Planned |
| URL-hash restore carries the resolved lineup | Planned |
| Playwright `setup.spec.ts` + copy updates ("8 AI bots" → dynamic) | Planned |

**Depends on:** nothing unmerged — builds directly on `main`/`EPIC-50` HEAD
(`89205c7`, 2026-07-15). **Related:** [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md)
(its difficulty selector shares the setup panel introduced here; when its YAML
bundles land, this EPIC's picker reads archetypes from bundle data instead of
the hardcoded registry).

---

## Context

Player mode is fixed at nine seats with a randomized lineup the player never
sees coming:

- `init_game()` hardcodes "9 players (seat 0 = human, seats 1-8 = bots)": it
  shuffles `default_profiles()` + `joker` and takes 8 (`src/lib.rs:81-96`).
  The eight archetypes plus joker are the full universe (pkcore
  `src/bot/profile.rs:491-502,465`).
- The UI has **no setup step at all** — the Play tab boots a table from a
  single "New Game" button (`www/index.html:81`, handler
  `www/js/main.js:439,419`), with two more entry points reusing the same
  fixed call: the inline `new-game` action (`www/js/main.js:776`) and the
  "New Table" walk-away button (`www/js/main.js:1054,1067`).
- Bot identity already surfaces per seat: emoji per archetype
  (`BOT_EMOJIS`, `www/js/main.js:27-37`) and a name-link to each bot's YAML
  config (`www/js/main.js:38-44`) — the player can *see* who they face, but
  never *choose*.

The machinery for variable-size tables is already paid for:

- pkcore tables are seat-count-agnostic — `Table::nlh_from_seats` takes any
  `Seats` (pkcore `src/casino/table.rs:144`), and its doctests exercise
  heads-up tables explicitly (`table.rs:475,514`).
- Short-handed play already happens **every session**: `next_hand()` calls
  `session.eliminate_busted()` (`src/lib.rs:465`), so late-game tables shrink
  toward heads-up today. Seat numbers stay stable through it, and bot lookup
  is positional — seat *N* → `BOTS[N-1]` (`src/lib.rs:1072-1073`) — so a
  shorter `BOTS` vec needs zero mapping changes.
- The renderer tolerates absent players: `renderSeat` early-returns to a
  dimmed `.seat-empty` placeholder (`www/js/table.js:157-166`,
  `www/css/table.css:45`), keyed by `bySeat.get(i)` (`www/js/table.js:270-271`).
  Nine seat nodes always exist (`www/js/table.js:105-109`); unused ones
  render as the same dimmed gaps eliminations already produce.
- A settings-persistence pattern exists: `pkarena.theme` / `pkarena.deck` /
  `pkarena.mobileSeats` in localStorage (`www/js/themes.js:2-4`).

The gap is purely a **configuration seam**: one new Thing (`TableConfig`),
its Business Requirements (count 1–8, known unique archetypes, honest
defaults, honest restore), and the Business Logic that seats the lineup.

**Not in this EPIC:** arena mode stays 9 bots (`src/lib.rs:141-149` untouched
— the ask is player mode); difficulty tiers and YAML lineups (EPIC-49);
re-spacing the oval for short tables (dimmed placeholders match existing
elimination rendering); blind/stack configuration (the `BLIND_LEVELS`
schedule at `www/js/main.js:979-992` is untouched).

## Goals

- The player picks their **opponent count** (1–8) — heads-up drills through
  full-ring — before dealing a hand.
- The player picks **which archetypes** they face, by name/emoji — including
  **duplicates** (a table of five maniacs is a legal, deliberate choice) —
  or keeps the default **random draw**.
- **Today's behavior is the default**, seeded-identically: `init_game(seed)`
  with no config produces the exact lineup it produces now, so existing
  URL-hash restores keep replaying truthfully.
- The choice **persists** (localStorage) and **restores** (URL hash carries
  the *resolved* lineup, so a shared/replayed session reseats the same bots).

## Scope

**In scope:** `TableConfig` parse/validate, archetype registry, new WASM
entry point, setup panel UI on the Play tab, persistence, hash restore,
Playwright coverage, copy updates (`www/index.html:8,78`,
`www/js/main.js:1131` all say "8 AI bots" / "9 bots").

**Out of scope:** everything in the "Not in this EPIC" list above.

---

## Design

### `TableConfig` + archetype registry (Rust)

`src/lib.rs` (new items near the profile pool code at `src/lib.rs:93-96`):

```rust
#[derive(Deserialize, Default)]
struct TableConfig {
    /// Number of bot opponents, 1..=8. Default 8 (today's table).
    opponents: Option<u8>,
    /// Archetype names to seat, in order, at seats 1..=lineup.len().
    /// Must be known names, len <= opponents. Repeats are allowed —
    /// duplicates are an explicit choice. Remaining seats are filled by
    /// a shuffled draw from the archetypes NOT named in the lineup, so
    /// duplicates never happen by accident.
    lineup: Option<Vec<String>>,
}

/// name → constructor, the 9 archetypes of default_profiles() + joker.
fn profile_by_name(name: &str) -> Option<BotProfile> { /* match */ }
```

Validation errors (unknown name, `opponents` out of 1..=8, `lineup` longer
than `opponents`) return the existing `error_state(..)` shape rather than
panicking — same contract as bad action JSON (`src/lib.rs:218-220`).

Rationale: a registry keyed by the names the UI already uses
(`BOT_EMOJIS` keys, `www/js/main.js:27-37`) means JS and Rust share one
vocabulary, and EPIC-49 can later swap the registry's backing store from
hardcoded constructors to YAML bundles without touching this API.

### `init_game_configured(rand_seed, config_json)` (WASM)

```rust
#[wasm_bindgen]
pub fn init_game_configured(rand_seed: f64, config_json: &str) -> String {
    // parse TableConfig; seat human + resolved lineup; deal.
}

#[wasm_bindgen]
pub fn init_game(rand_seed: f64) -> String {
    // becomes: init_game_configured(rand_seed, "{}")
}
```

The **default path must reproduce today's RNG call sequence exactly**
(shuffle the 9-profile pool, take 8 — `src/lib.rs:93-96`): a seeded
equivalence test pins it, because existing `#mode=…&seed=…` hash restores
(`www/js/main.js:280`) replay through `init_game(seed)` and must keep
producing the same table. Explicit lineups seat chosen names in order at
seats 1..k, then fill to `opponents` from the shuffled remainder — the
resolved lineup is echoed in the returned `GameState` so JS can pin it in
the URL hash.

### Duplicate archetypes: unique handles, archetype-keyed display

Two `BotProfile`s can share a name (the profile drives decisions
positionally — seat *N* → `BOTS[N-1]`, `src/lib.rs:1072-1073` — so the
engine doesn't care), but the seat `Player` handle must stay unique for
readable hand histories and showdown logs. At seating time, repeated names
get an ordinal suffix: `maniac`, `maniac 2`, `maniac 3`
(`Player::new_with_chips` call sites, `src/lib.rs:99-108`).

The UI currently keys emoji and config-link off the raw seat name —
`emoji: name => BOT_EMOJIS[name]` and `nameHref: botConfigUrl`
(`www/js/main.js:487-488`) — which a suffixed handle would miss. Add one
JS helper, `botArchetype(name)` (strip a trailing ` <digits>`), and route
both callbacks through it. `table.js` needs no change — it already just
calls the injected callbacks (`www/js/table.js:170-174`). The URL-hash
`lineup=` csv carries **base archetype names** (`maniac,maniac,gto`), never
suffixed handles; suffixes are re-derived deterministically on restore.

### Setup panel (Play tab)

A "Table setup" section next to `#btn-new-game` (`www/index.html:81`),
following the settings-overlay markup style (`www/index.html:109-138`):

- **Opponents**: stepper/select, 1–8, default 8.
- **Lineup**: nine archetype chips (emoji + name from `BOT_EMOJIS`,
  `www/js/main.js:27-37`), each with a per-chip count (tap to add, long-tap
  or a small − to remove) so duplicates are first-class — e.g. `💣 maniac
  ×3`. Default all zero = "random draw". Total chip count k ≤ opponents n;
  the k chosen seat first (in pick order), the rest draw randomly from the
  unchosen archetypes.

Choice is stored as JSON under `pkarena.tableConfig` (pattern:
`www/js/themes.js:2-4`) and read by **all three** game-start paths
(`www/js/main.js:419,776,1067`), which switch from `init_game(seed)` to
`init_game_configured(seed, cfg)` whenever a non-default config exists.

### URL-hash restore

The hash currently carries `#mode=…&seed=…&hand=…` (`www/js/main.js:206-207`)
and restore replays `init_game(seed)` (`www/js/main.js:280`). Add a
`lineup=<csv>` param holding the **resolved** lineup (post random fill, as
echoed by the WASM). Restore passes it back as an exact `TableConfig`
(`opponents` = list length), so a shared link reseats the same bots even
though localStorage may have changed since. Absent `lineup=` ⇒ legacy
default path ⇒ old links keep working.

---

## Work Items

### Phase 0 — Config seam (Rust, no behavior change)
- [ ] 0a. `TableConfig` + `profile_by_name` registry + lineup resolution in
  `src/lib.rs`; native unit tests for parse/validate/fill rules.
- [ ] 0b. Refactor `init_game` internals through the resolver with the
  default config; seeded test proves lineup-identical output vs `89205c7`
  behavior (same seed → same bot names in same seats).

### Phase 1 — WASM API
- [ ] 1a. Export `init_game_configured`; error paths through
  `error_state()`; resolved lineup echoed in `GameState`.
- [ ] 1b. Seeded heads-up test: config `{opponents: 1, lineup: ["maniac"]}`
  plays a full hand to completion (blinds, showdown, `next_hand`) — pins the
  pkcore heads-up support (`table.rs:475,514`) end-to-end through this app.
- [ ] 1c. Seeded partial-lineup test: 5 opponents, 2 named — seats 1–2 match
  the names in order, seats 3–5 drawn from the 7 archetypes not named, so
  the random fill never introduces a duplicate.
- [ ] 1d. Duplicates test: `{opponents: 3, lineup: ["maniac", "maniac",
  "maniac"]}` — three maniac profiles seated, handles `maniac` / `maniac 2`
  / `maniac 3`, a full hand plays, and the hand history shows three
  distinct handles.

### Phase 2 — Setup UI + persistence
- [ ] 2a. Table-setup panel markup/CSS on the Play tab
  (`www/index.html:81` area) with count control + archetype chips.
- [ ] 2b. `pkarena.tableConfig` read/write (themes.js pattern); wire all
  three start paths (`www/js/main.js:419,776,1067`).
- [ ] 2b′. `botArchetype(name)` helper; route the `emoji` / `nameHref`
  callbacks (`www/js/main.js:487-488`) through it so `maniac 2` still gets
  💣 and links to `maniac.yaml`.
- [ ] 2c. `lineup=` URL-hash param: written on game start, honored by
  `restoreFromUrl` (`www/js/main.js:265-280`); legacy hashes unaffected.
- [ ] 2d. Copy updates: `www/index.html:8,78`, `www/js/main.js:1131` no
  longer hardcode "8 AI bots" / "9 bots".

### Phase 3 — E2E + docs
- [ ] 3a. `tests/setup.spec.ts`: pick 2 opponents incl. `maniac` → seat 1–2
  named/emoji'd correctly, seats 3–8 render `.seat-empty`; config survives
  reload; hash link reproduces the lineup in a fresh context; a ×2 duplicate
  pick renders `maniac` and `maniac 2` with the same emoji and config link.
- [ ] 3b. Confirm existing specs stay green unmodified — they assert 9 seat
  *nodes*, not 9 players (`tests/ui.spec.ts:11-16`, `tests/table.spec.ts:37`,
  `tests/mobile.spec.ts:50`, `tests/replay.spec.ts:16`), and the default
  path is unchanged (`tests/helpers.ts:9-16` seeds and clicks New Game).
- [ ] 3c. Add EPIC-51 row to `docs/EPICS.md`.

---

## Key Files

| File | Role |
|---|---|
| `src/lib.rs:81-134` | `init_game` → configured path; `TableConfig`; registry |
| `src/lib.rs:1072-1073` | positional seat→bot mapping (unchanged, verified) |
| `www/index.html:81` | Play tab — setup panel mounts here |
| `www/js/main.js:419,776,1067` | the three game-start paths to wire |
| `www/js/main.js:27-44` | `BOT_EMOJIS` / `botConfigUrl` — the shared archetype vocabulary |
| `www/js/main.js:487-488` | `emoji` / `nameHref` callbacks → route through `botArchetype()` |
| `www/js/main.js:206-280` | URL-hash write/restore — add `lineup=` |
| `www/js/themes.js:2-4` | localStorage pattern to follow |
| `www/js/table.js:157-166,270-271` | empty-seat rendering (reused as-is) |
| `tests/setup.spec.ts` (new) | E2E acceptance |
| pkcore `src/bot/profile.rs:284-502` | archetype constructors (reused, not copied) |

## Reuse (do NOT recreate)

- `www/js/table.js:157-166` — `.seat-empty` placeholder rendering already
  handles absent players; short tables need no renderer change.
- `src/lib.rs:465` — `eliminate_busted()` already runs short-handed tables
  every session; no engine change for fewer starting players.
- `www/js/main.js:27-44` — `BOT_EMOJIS` + `botConfigUrl` are the picker's
  display layer.
- pkcore `src/bot/profile.rs:491-502,465` — the nine constructors; the
  registry maps names to them, nothing is duplicated.

## Compatibility

- **Preserves** `init_game(seed)`'s signature and seeded output (equivalence
  test), all existing localStorage keys, legacy URL hashes, arena mode, and
  every existing Playwright spec unmodified. **Adds**
  `init_game_configured`, `pkarena.tableConfig`, `lineup=` hash param, the
  setup panel. **Breaks** nothing.

---

## Verification

```bash
cargo test                       # config validation, seeded equivalence, heads-up, partial lineup
cargo clippy -- -D warnings
make build && npx playwright test   # setup.spec.ts + full existing suite green
```

Acceptance: (1) default New Game is seed-identical to `89205c7` behavior;
(2) a 1-opponent game plays hands to completion; (3) chosen archetypes sit
at seats 1..k in order — repeats included, with unique suffixed handles —
and random fill never introduces a duplicate; (4) config survives reload
and a shared hash link reseats the exact lineup, duplicates and all; (5)
all pre-existing specs pass unmodified.

---

## Open Questions

- **Arena parity.** Should arena mode gain the same picker? Cheap to add
  once the panel exists, but out of the stated ask; revisit with EPIC-49's
  arena chips/100 harness, where a *fixed* chosen lineup is genuinely useful.
- **Short-table layout.** Dimmed gaps vs re-spacing seats around the oval
  for 2–4 players. Gaps match today's elimination look; re-spacing is a
  cosmetic follow-on (`www/js/table.js:7-17` `SEAT_POS` would need per-count
  variants).
