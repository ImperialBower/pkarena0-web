# EPIC-56: The Embed Build — `<pk-table>` for other people's pages

> **One-line:** A second entry point beside `www/index.html`: one ES module,
> one stylesheet, and the same wasm, released as a tarball per tag, that mounts
> a playable hero-vs-eight-bots table into any element of any page — no fonts
> from Google, no top bar, no log, no audio, nothing fetched until the reader
> presses **Deal**. The first consumer is the HTML edition of *Rust for
> Failures* (bower
> [EPIC-19](../../bower/docs/EPIC-19_Table.md), which owns the contract).

> **Numbering:** continues this repo's own run (46–55, `EPICS.md`). pkcore's
> `ROADMAP.md:416` assigns EPIC-50–59 to `pkgate`, so 50–55 here already
> overlap it; registering a pkarena0-web block there is item 0c, not a reason
> to break the local sequence.

## Status

**Phases 0–3 done 18 October 2026; Phase 4 (the release) next.** Drafted the same day at `0884ca8` (v0.2.0).

| Component | Status |
|---|---|
| Phase 0 — decisions: Shadow DOM, the action-row extraction, the block registration | **Complete** — 18 October 2026, commit pending: `tests/shadow.spec.ts` green; pkcore `ROADMAP.md` line added; `bots` reserved in § Design |
| Phase 1 — `www/js/actions.js`: `buttonsFor(state)` lifted out of `main.js`, parity-tested | **Complete** — 18 October 2026, commit pending: `tests/actions.spec.ts` four green; full suite 70/70 |
| Phase 2 — `www/js/embed/pk-table.js`, `www/css/pk-table.css`, `www/embed.html` | **Complete** — 18 October 2026, commit pending |
| Phase 3 — `tests/embed.spec.ts`: the eight contract rules | **Complete** — 18 October 2026, commit pending: 10 green (eight rules, a seed rule, script off); full suite 79/79 |
| Phase 4 — `release.yml`: `pk-table-<tag>.tar.gz` + `MANIFEST.sha256` on every tag; size gate | Planned |
| Phase 5 — README "Embedding", CHANGELOG, `EPICS.md`, the isolation-trick comment, the tag | Planned |

**Depends on:** nothing unmerged. Builds on `main` at v0.2.0 (`MIT OR
Apache-2.0`, which is what let the consumer vendor it — bower EPIC-19
decision 0a).
**Related:** [EPIC-54](EPIC-54_Player_Mode_Table_Setup.md) (`bots` is
reserved for its `init_game_configured`); bower EPIC-18 (measured a 2.19 MB
wasm and ruled out an iframe for a book, which is why this is a module and
not a URL).

---

## Context

### What exists

- **The engine is already a component.** All state is `thread_local!`
  (`src/lib.rs:69-80`); the page drives it through `init_game(seed)`
  (`:278`), `step_bot()` (`:1365`), `human_action(json)` (`:409`),
  `next_hand()` (`:501`), `get_state()` (`:773`) and `version()` (`:215`),
  every one returning JSON. `build_game_state()` (`:1532`) already ships
  `hero`, `players`, `board`, `pot`, `to_call`, `min_raise`, `max_bet`,
  `legal_actions` (`:1596-1598`, from `Table::legal_actions`), `can_undo`,
  `hand_number`, `phase`. Nothing new needs plumbing.
- **The drawing is already a component.** `createTable(rootEl, { replay })`
  (`www/js/table.js:111`) builds felt, nine seats, board and pot inside the
  element it is given, "never clears rootEl — it only appends"
  (`table.js:1-2`), and returns `{ update, setActionLabel, clear }`
  (`:325`). `makeCard` (`www/js/cards.js:4`) draws a card from `"Kh"` or
  `"__"`. `tests/table.spec.ts` already mounts `createTable` into a bare
  `<div>` and asserts nine seats and five slots — the embed's first test
  exists in spirit.
- **Two engines on one page is a solved problem.** `boot()` imports the glue
  twice, `?tab=play` and `?tab=arena` (`www/js/main.js:321-322`); the module
  cache keys on the full URL, so each gets its own `thread_local!` state.
  `docs/TECHNICAL_DEBT.md:55` flags that this invariant is uncommented. The
  embed makes it a documented feature: one import per element.
- **The pacing is two numbers.** `BOT_ACTION_MS = 1000` and
  `HAND_COMPLETE_MS = 5000` (`main.js:16-17`), zeroed by the test hook
  `setInstant` (`main.js:342`).

### What does not exist

- **A controller that is not the page.** `main.js` is 68 KB, reaches into
  the document by id 76 times, and owns the top bar, tabs, arena, replay,
  log, settings, voice, P&L in `localStorage` (`:54`), the URL hash (`:240`),
  and `speechSynthesis` (`:1289`). The action-row logic the embed needs,
  `renderActionButtons(state)` (`main.js:852-900`), is pure in content —
  legal actions in, labelled buttons out — but it lives inside that closure
  and writes to `#action-buttons` (`:903`).
- **A stylesheet that stands alone.** `table.css` (8.7 KB) draws the table
  but reads `--font-mono` and the colour ramps from `tokens.css` (13.4 KB,
  four themes) and the card rules from `organic-ds.css`; `index.html:16-18`
  loads five Google Fonts those tokens name. A host page offline gets the
  fallback fonts anyway; the embed should ask for nothing else.
- **A release artefact.** `deploy.yml` builds the release wasm and uploads
  `www/` as a Pages artifact; `v0.2.0` and `v0.1.0` are the only releases
  and neither has assets. A consumer that wants to *pin* a build has only a
  tag to clone and `wasm-pack` to run.

### Measured, before any work

Local `www/pkg/` (built 2 September 2026 from the v0.1.28 tree with
`wasm-pack build --release`; v0.2.0 changed no code, per its CHANGELOG):

| file | raw | gzip -9 | brotli -q 11 |
|---|---|---|---|
| `pkarena0_web_bg.wasm` | 2,179,276 | 687,744 | 482,903 |
| `pkarena0_web.js` | 18,486 | 4,684 | 4,031 |

The consumer's budget is ≤ 2.3 MB raw / ≤ 600 KB brotli for the wasm and
≤ 60 KB for `pk-table.js` + `pk-table.css` together. Enabling `hup-charts`
by accident took the download to 3.84 MB once (CHANGELOG 0.1.28); the
release job asserts the size so it cannot happen silently.

### The kata

**Things:** the Table (an engine instance plus a drawing), the Mount (the
host's element and the picture it already holds), the Release (seven files
and a manifest, named by tag). **Requirements:** nothing fetched before
Deal; one origin; failure leaves the picture; each element its own engine;
the version on the page is the tag. **Logic:** one custom element, one pure
function for the action row, one workflow.

---

## Goals

- A host page writes two lines — a `<script type="module">` and a
  `<pk-table>` wrapping an `<img>` — and gets a hand of NLHE against the
  v0.2.0 bots, with **nothing fetched until Deal** and **nothing fetched from
  any origin but the module's own**.
- The embed **degrades to the picture**: `file://`, script off, a wasm that
  fails to load, or an old browser leaves the host's `<img>` and a sentence,
  never a spinner.
- **Each element is its own table.** Two on one page deal two hands.
- The build is **pinnable**: a tarball per tag with a `MANIFEST.sha256`, and
  `version()` shown on the element equals the tag.
- The full app **does not change behaviour**: every existing Playwright spec
  stays green; `main.js` loses the action-row logic to a module and nothing
  else.

## Scope

- **In:** felt, nine seats with stacks and bets, board, pot, the hero's
  cards, the status line (`aria-live="polite"`), the action row (Fold /
  Check / Call / Min / ½ Pot / Pot / `Raise…` with a slider / All-In / Undo,
  exactly `renderActionButtons`'s set), **Deal** and **Next hand**, a
  one-line badge `pkarena0-web vX.Y.Z`.
- **Out:** top bar and tabs, Arena, Replay, hand log and YAML export,
  settings, theme select and deck toggle, voice and audio, P&L, HUD,
  pre-action Check/Fold, `localStorage`, the URL hash, the PWA manifest,
  Google Fonts. Bot count (EPIC-54) and difficulty stay at today's defaults:
  eight bots, blinds 50/100, standard tier.
- **Reserved:** the `bots` attribute. Accepted, ignored with one
  `console.warn`, until EPIC-54 ships `init_game_configured`.

---

## Design

### Element

```html
<script type="module" src="pk-table.js"></script>
<pk-table seed="42" delay="1000">
  <img src="table.png" alt="A poker table, hero's cards dealt">
</pk-table>
```

| Attribute | Meaning | Default |
|---|---|---|
| `base` | URL prefix for `pkarena0_web.js`, `pkarena0_web_bg.wasm`, `pk-table.css`; resolved against the document | the module's own directory (`import.meta.url`) |
| `seed` | passed to `init_game(seed)`, which seeds **the bot lineup** (`src/lib.rs:281-289` shuffles the profile pool with it). It does not pin the cards: pkcore's `start_hand` shuffles from entropy (`lib.rs:3617`). A number | fresh per Deal |
| `css` | URL of `pk-table.css`, resolved against the document | beside the module (the release layout; a checkout keeps it under `css/`) |
| `delay` | ms between bot actions (`BOT_ACTION_MS`); `0` for tests | `1000` |
| `bots` | **reserved** (EPIC-54) | `8` |

Events, all `bubbles: true, composed: true`: `pk-table:ready`
(`detail.version`), `pk-table:hand-over` (`detail.state`, the `get_state`
JSON), `pk-table:error` (`detail.message`).

### The eight rules — the contract, each a test

1. **Nothing until Deal.** `connectedCallback` fetches nothing. The light-DOM
   `<img>` is slotted and visible; the element adds a **Deal** button and an
   empty status line under it.
2. **Each element its own engine.** Deal imports
   `new URL('pkarena0_web.js?instance=' + id, base)` — the `main.js:321-322`
   trick, now written down — and inits with
   `{ module_or_path: new URL('pkarena0_web_bg.wasm', base) }`. The wasm
   bytes come from the HTTP cache the second time; the instance is new.
3. **Failure keeps the picture.** A rejected import or init leaves the
   `<img>`, disables Deal, and writes one sentence to the status line:
   `file://` → "needs to be served over http"; otherwise "the table did not
   load". `pk-table:error` fires. No spinner outlives a failure.
4. **One origin.** Every request the element causes is `base`-relative.
   `pk-table.css` names no `url()` and no `@import`; fonts are
   `ui-monospace, Menlo, Consolas, monospace` and `system-ui, sans-serif`.
5. **The loop is the app's.** After Deal, `step_bot()` every `delay` ms until
   `phase` is `WaitingForHuman`, `HandComplete`, or `SessionOver`
   (`main.js:199-230` is the reference). `HandComplete` shows the result and
   **Next hand**; `SessionOver` shows the result and **Deal** again (a fresh
   `init_game`).
6. **Focus survives a render.** The action row is rebuilt from
   `buttonsFor(state)`; if the focused button's `data-act` exists in the new
   row, focus moves to it, else to the first button. Deal is the first
   focusable thing in the element.
7. **Styles are scoped and themeable.** Shadow DOM; `pk-table.css` is
   fetched once per page (from `base`) and attached with
   `adoptedStyleSheets`, falling back to a `<link>` inside the shadow root.
   The surround reads `--pk-bg`, `--pk-fg`, `--pk-accent`, each defaulting
   to the host's `--bg` / `--fg` / `--links` (mdBook's names) and then to the
   midnight theme's values. The felt is green on every host.
8. **The badge is the tag.** The element shows `pkarena0-web v` +
   `version()`; the release job fails if `version()` ≠ the tag being
   released.

### `www/js/actions.js` — the first module split off `main.js`

```js
// Pure: GameState JSON → the action row. No DOM, no closure state.
export function buttonsFor(state) → [{ label, cls, action, amount?, betAction? }]
```

(The field is `action`, as `renderButtons` already reads it — the sketch's
`act` would have meant touching the dock's renderer for no gain.)

The body is `renderActionButtons` (`main.js:856-897`) minus the pre-fold
check at `:855` and the `renderButtons` call at `:899`. `main.js` imports it
and keeps rendering as before; the embed renders it into its own row. One
parity test pins the old output for three states (facing a bet, option to
check, short stack) so the extraction cannot change the app.

### `www/js/embed/pk-table.js`

```js
import { createTable } from '../table.js';
import { makeCard }    from '../cards.js';
import { buttonsFor }  from '../actions.js';

let nextInstance = 0;
export class PkTable extends HTMLElement {
  static observedAttributes = ['seed', 'delay'];
  #mod; #table; #id = nextInstance++;
  connectedCallback() { /* shadow root, <slot>, Deal, status, badge; no fetch */ }
  async deal() { /* rule 2, then init_game, render, loop (rule 5) */ }
  // onAction(act, amount): human_action({action, amount}) as main.js:519, then loop
}
customElements.define('pk-table', PkTable);
```

Target size: `pk-table.js` ≤ 12 KB, `pk-table.css` ≤ 30 KB (table.css +
the card rules + one theme's tokens + the row and the surround). As built
the module is 16.6 KB raw — over its own line, mostly comments and the
raise strip — and the pair is 30 KB raw / 8 KB brotli against the
consumer's binding budget of 60 KB (bower EPIC-19 § Scope). Left as is.

### `www/embed.html`

A page of prose with one `<pk-table>` in the middle and a second further
down, served by Pages beside `index.html`. It is the thing Playwright tests
and the thing a consumer copies from.

### The release

`.github/workflows/release.yml`, on `push: tags: ['v*']`:

```
wasm-pack build --release --target web --no-typescript --no-pack --out-dir dist/pkg
assert: version() in the glue == ${GITHUB_REF_NAME#v}      (rule 8)
assert: pkarena0_web_bg.wasm < 2,300,000 bytes              (the hup-charts tripwire)
stage: pk-table.js pk-table.css pkarena0_web.js pkarena0_web_bg.wasm LICENSE-MIT LICENSE-APACHE
sha256sum * > MANIFEST.sha256
tar czf pk-table-$TAG.tar.gz -C stage .
gh release upload $TAG pk-table-$TAG.tar.gz --clobber   (create the release if missing)
```

`pk-table.js`'s relative imports (`../table.js`, `../cards.js`,
`../actions.js`) are inlined into the staged copy by a small
`scripts/embed-bundle.mjs` (string concatenation of four ES modules; no
bundler, so the query-string isolation of rule 2 cannot be normalised away —
the `TECHNICAL_DEBT.md:55` worry). `deploy.yml` is untouched.

---

## Phase 0 — Decisions

- [x] **0a.** Shadow DOM (rule 7) over scoped classes. `table.js` and
  `cards.js` make no `document.getElementById` / `querySelector` / `window`
  lookups (grep, 18 October 2026). `tests/shadow.spec.ts` mounts
  `createTable` under an open shadow root with `table.css` linked inside it:
  nine seats, five slots, three board cards and the pot render; nothing lands
  in the host's light DOM; the felt is `position: absolute`, so the
  stylesheet reached it. Green against `main` at `ccb874e`.
- [x] **0b.** `bots` reserved in § Design's attribute table (ignored with
  one `console.warn` until EPIC-54). The README copy is 5a.
- [x] **0c.** pkcore `ROADMAP.md` § EPIC Numbering Policy gains a
  pkarena0-web line (18 October 2026, in the pkcore clone, commit pending):
  50–56 recorded as overlapping `pkgate`'s block, both runs keep their
  numbers, and **EPIC-100–109** is claimed as pkarena0-web's next block, so
  the EPIC after this one is `EPIC-100`.

## Phase 1 — `actions.js`

- [x] **1a.** `www/js/actions.js` holds `buttonsFor(state)`, the 44 lines
  of `renderActionButtons`'s body verbatim; `main.js` imports it and
  `renderActionButtons` is now the pre-fold check plus
  `renderButtons(buttonsFor(state), state)`. `tests/actions.spec.ts` pins
  four states — facing a bet (½ pot suppressed below the min raise), option
  to check, a short stack whose call is clamped and marked all-in with
  Undo, and no legal actions — with expected rows derived from the code at
  `ccb874e` before the lift. Red first (module missing), then green.
- [x] **1b.** `npx playwright test`: 70 passed, 0 failed (18 October 2026,
  1.4 min), the 66 pre-existing specs plus `shadow.spec.ts` and the four
  in `actions.spec.ts`.

## Phase 2 — The element

- [x] **2a.** `www/js/embed/pk-table.js` per § Design; `www/css/pk-table.css`
  (a `:host` token block, the surround, then `table.css` verbatim — a
  bundle step in Phase 4 keeps the copy in step); `www/embed.html` with two
  tables and `www/img/table-still.svg` (the repo's `img/poker-table.svg`)
  as the picture. Found by the first screenshot: the still stayed above the
  live table, so `.pk-live` now hides the slot once the engine is up and
  rule 7 asserts it. Found by the second: the same seed dealt different
  cards — `seed` seeds the lineup, not the deck; the attribute table, the
  demo page, bower EPIC-19 and rule 2 now say so.
- [ ] **2b.** Comment the import site (`main.js:321`) with the invariant
  (the embed's `#load` already carries it); close `TECHNICAL_DEBT.md:55`'s
  first suggestion. *(Deferred to Phase 4 with the bundle step, which is
  where a bundler would break it.)*
- [x] **2c.** Sizes in § Measured: 16,606 / 13,418 raw; 4,896 / 3,137
  brotli.

## Phase 3 — `tests/embed.spec.ts`

- [x] **3a.** One `test` per rule, against `/embed.html`: request log empty
  for `pkg/` before Deal (1); two elements, two `hand_number`s and
  different hero cards for different seeds (2); `route('**/*.wasm', abort)`
  → `<img>` present, Deal disabled, message, `pk-table:error` (3); set of
  request origins has size one (4); a hand to `HandComplete` with `delay=0`
  (5); focus Fold, force a render, `activeElement` still in the row (6);
  computed font-family contains no Google name; `--pk-bg` follows a host
  `--bg` (7); badge text equals `CARGO_PKG_VERSION` read from `Cargo.toml`
  (8). Plus 2b: the same seed seats the same bots. Written first: nine
  red against a page that did not exist, then green one fix at a time —
  the dev layout needed `base="pkg/"` and the new `css` attribute, and the
  local `www/pkg` had to be rebuilt at 0.2.0 for rule 8.
- [x] **3b.** Script-off run (`javaScriptEnabled: false`): the `<img>` and
  nothing else inside `pk-table`.

## Phase 4 — The release

- [ ] **4a.** `scripts/embed-bundle.mjs`; `release.yml` as § Design; the
  two asserts.
- [ ] **4b.** Dry run on a `v0.2.1-rc` tag of a branch (delete after);
  download, `sha256sum -c MANIFEST.sha256`, serve the directory, open a
  page that mounts it: a hand plays.
- [ ] **4c.** Tag `v0.2.1`; the release carries `pk-table-v0.2.1.tar.gz`.

## Phase 5 — Docs

- [ ] **5a.** README § Embedding: the two lines, the attribute table, the
  tarball, the license. CHANGELOG 0.2.1.
- [ ] **5b.** `EPICS.md` row flipped; this Status table flipped with
  commits; § Measured filled.

---

## Measured

Filled by Phases 2 and 4.

| | raw | brotli | budget |
|---|---|---|---|
| `pkarena0_web_bg.wasm` (release, v0.2.1) | | | ≤ 2.3 MB / ≤ 600 KB |
| `pkarena0_web.js` | | | |
| `pk-table.js` (checkout, unbundled) | 16,606 | 4,896 | ≤ 12 KB — missed; pair ≤ 60 KB holds |
| `pk-table.css` | 13,418 | 3,137 | ≤ 30 KB |
| `pkarena0_web_bg.wasm` (dev build, 18 Oct) | 2,156,142 | | release numbers in Phase 4 |
| requests before Deal | 0 | | 0 (rule 1) |
| Deal → first bot action, `delay=1000` | | | |

## Verification

```bash
make validate-bots build                 # unchanged
npx playwright test                      # every spec, old and new
npx playwright test tests/embed.spec.ts tests/actions.spec.ts
# the artefact, after 4c
gh release download v0.2.1 -p 'pk-table-*.tar.gz' -D /tmp/pk && cd /tmp/pk && tar xzf *.tar.gz && sha256sum -c MANIFEST.sha256
grep -lE 'https?://' pk-table.js pk-table.css || echo "no absolute URLs"
ls -l pkarena0_web_bg.wasm
```

Exit criteria:

1. `/embed.html` plays a hand to `HandComplete` in Chromium with every
   request on one origin and none before Deal.
2. Two `<pk-table>`s on `/embed.html` run independent hands.
3. With the wasm blocked, the picture stays and the element says so.
4. Every pre-existing spec is green after the `actions.js` lift.
5. `v0.2.1`'s release carries the tarball; its manifest verifies; the
   badge reads `v0.2.1`; the wasm is under 2.3 MB.
6. README documents the two lines and the attributes; `bots` is listed as
   reserved.
