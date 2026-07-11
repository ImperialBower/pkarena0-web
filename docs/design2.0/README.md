# Handoff: pkarena Poker Table UI Redesign

## Overview
A professional redesign of the pkarena web demo (https://imperialbower.github.io/pkarena0-web/) — the No-Limit Hold'em Play view: table, seats, action buttons, hand log, and top status bar. The design ships with **four switchable themes** selected from a dropdown in the top bar: `midnight` (dark modern client), `terminal` (light monospace/blueprint), `luxe` (charcoal & gold), and `organic` (warm cream/terracotta). The theme should be a runtime setting persisted per user.

## About the Design Files
The files in this bundle are **design references created in HTML** — an interactive prototype showing the intended look and behavior, not production code to copy directly. The task is to **recreate this design in the pkarena0-web codebase's existing environment**, using its established patterns, state management, and build setup. Do not port the prototype's rendering runtime (`support.js`) — it is a preview harness only.

Open `Poker Table.dc.html` in a browser (from this folder, with `support.js` and `_ds/` alongside) to see and interact with the live reference. All theme values live in one place in that file: the `themes()` method near the bottom returns a `{ midnight, terminal, luxe, organic }` object where every key is a full palette/style spec — treat it as the source of truth for exact values.

## Fidelity
**High-fidelity.** Colors, typography, spacing, radii and states are final. Recreate pixel-perfectly.

## Architecture recommendation
Implement theming as a token layer: one set of semantic CSS custom properties (`--bg`, `--panel`, `--line`, `--text`, `--dim`, `--accent`, `--accent-text`, `--danger`, `--bet`, `--felt-bg`, `--felt-border`, `--plate-bg`, `--radius-btn`, `--radius-pill`, `--font-body`, `--font-mono`, `--font-display`, …) with four theme classes (e.g. `body.theme-midnight`) that assign values. Components reference only the semantic tokens. The `themes()` object in the HTML file maps 1:1 onto this.

## Layout (all themes share it)
Full-viewport column: **top bar (52px)** → **main row (flex:1)** → optional **raise strip** → **action dock**.

### Top bar (height 52px, horizontal flex, gap ~13px, padding 0 18px)
- Brand "PKARENA" + version `v0.1.12` (mono, 9px, faint).
- Tabs: PLAY (active) / ARENA. Active = filled pill (midnight/organic), inverted box (terminal), gold underline (luxe).
- Right cluster (mono 10.5px): `HAND 1/100 · STACK $10,000 · P&L +$14,933` (P&L in the theme's "good" green), divider, **theme dropdown**, **2/4-color deck toggle**, **LOG toggle**, **New Table** button.

### Main row
- **Table zone** (flex:1, relative): the felt is an absolutely-positioned surface at `left/right: 6%, top: 8%, bottom: 11%` — stadium shape (border-radius 280px) in midnight/terminal/organic, ellipse (50%) in luxe — with a concentric inner hairline ring inset 10–16px.
- **Center of table** (absolute, 50% / 47%): pot pill `POT $1,500`, five empty board-card slots (36×50px, dashed borders, micro labels FLOP/FLOP/FLOP/TURN/RIVER), table label `NO LIMIT HOLD'EM · 50/100` (letter-spaced, low-contrast).
- **9 seats** absolutely positioned by percentage (center-anchored via translate(-50%,-50%)):
  BTN/hero 71,90 · SB 29,90 · BB 8,64 · UTG 8,30 · UTG+1 27,7 · MP 50,4 · LJ 73,7 · HJ 92,30 · CO 92,64 (x%, y%).
- **Hand log** (right aside, 248px, collapsible): header `HAND LOG` + ✕ close, scrollable numbered mono entries (9.5px), footer with `EXPORT YAML` and `REPLAY` buttons. Toggled from the top bar LOG button.

### Seat plate (min-width ~120px)
- Row 1: player name (bold; serif in luxe) + position tag (mono 8.5px, dim, right-aligned).
- Row 2: stack `$9,600` (mono 11px semibold) + `96BB` (8.5px dim).
- Row 3: HUD stats `VPIP 58 · PFR 44 · AF 4.6` (mono 8px, faint) — hideable via a setting.
- Below plate: **action pill** (8.5px caps mono, pill/square per theme): FOLD (muted), RAISE $400 (danger/red tint), POST $50 (neutral), TO ACT (accent).
- Below that: **bet amount chip** when the player has chips in front ($400, $950, $50, $100).
- **Dealer/blind badge**: 18px circle overlapping plate top-right (D / SB / BB).
- Folded seats: whole seat at 42% opacity.
- Hero seat: 2px accent ring + soft accent glow around the plate.

### Raise strip (appears above dock when "Raise…" is toggled)
`RAISE TO` label · amount (mono 17px bold) · range slider (min 1500, max 10000, step 25, accent-colored) · quick buttons `MIN / 3× / POT / ALL-IN` · confirm button `Raise to $X` (accent fill).

### Action dock (bottom bar, padding 14px 18px)
- Hero hole cards: two 48×66px cards (white/cream face), rank 19px + suit 15px, mono bold.
- Hero info: `You · BTN · $10,000 (100 BB)` and sub-line `KTo · TO CALL $950 · POT ODDS 39% · SPR 4.1` (mono 9.5px dim).
- Buttons right (gap 10px): **Fold** (danger outline) · **Call $950** (solid primary) · **Min $1,500** (ghost) · **Raise…** (accent outline/tint, opens strip) · **All-In $10,000** (warning/danger outline).

## Mobile (breakpoint < 760px)
The reference file is responsive — narrow the browser window below 760px to see it. One template, two layouts:
- **Compact top bar (48px)**: logo · `H 1/100` · P&L · theme select · seat-view toggle (`LIST ⇄` / `TABLE ⇄`) · LOG.
- **Two seat treatments** (user-switchable; also a `mobileSeats` setting):
  1. **Mini table** — portrait stadium/oval felt (left/right 5%, top 4%, bottom 5%), condensed seat plates (name + stack only, ~62px min-width, 9px type), abbreviated action pills (`FOLD`, `R $400`, `SB $50`, `TO ACT`), centered pot pill + 5 small board slots (26×36px).
  2. **Seat list** — scrollable column of rows (9px 12px padding): position tag (32px) · name + HUD stats · stack + BB (right-aligned) · action pill (58px). Hero row gets a 1.5px accent border; folded rows at 50% opacity. A board strip (pot · 5 mini slots · blinds) sits on top.
- **Hand log = bottom drawer**: fixed sheet at 62vh with drag handle, backdrop tap-to-close, same log content/footer. Rounded top corners except in `terminal` (square).
- **Action dock**: hero cards (38×52px) + condensed info + 2/4-color toggle on one row; below it a horizontal row of three large buttons — `Fold` (flex 1) · `Call $950` (flex 1.3) · `Raise…` (flex 1) — 15px block padding (≈48px tall, thumb-safe). `Min` and `All-In` move into the raise strip's quick buttons (`MIN / 3× / POT / ALL-IN`), which stacks as two rows: slider row + quick-buttons row with confirm.
- Implementation note: the prototype detects `window.innerWidth < 760`; in production use a CSS media query or container query on the same breakpoint.

## Interactions & Behavior
- **Theme dropdown**: instantly restyles the entire UI; persist choice (localStorage or user prefs).
- **2/4-color deck toggle**: 2-color = hearts/diamonds `#C63C4C`, spades/clubs `#20242C`. 4-color = diamonds `#2E5FD0`, clubs `#1F8A54`, hearts red, spades black. Applies to all card pips.
- **LOG toggle / ✕**: shows/hides the hand-log aside; table zone reflows to fill.
- **Raise…**: toggles the raise strip; slider and quick buttons update the amount live; confirm submits `raise to <amount>`.
- Hover states: subtle tint of the element's own color family (accent-tinted background for outlined buttons, one ramp step darker for filled buttons). Keyboard focus: 2px accent outline, offset 2px.
- Card slots fill left-to-right as streets are dealt (board cards replace the dashed placeholders at the same size).

## State Management
- `theme: 'midnight' | 'terminal' | 'luxe' | 'organic'`
- `fourColorDeck: boolean`
- `handLogOpen: boolean`
- `raisePanelOpen: boolean`, `raiseAmount: number`
- `showHud: boolean` (per-seat stats visibility)
- Game state (seats, stacks, actions, pot, board, hero cards, to-call, pot odds, SPR) comes from the existing pkcore engine; the prototype's data is a hardcoded snapshot of hand #1 preflop.

## Design Tokens (exact values — see `themes()` in the HTML for the complete set)

### Fonts (Google Fonts)
- midnight: Space Grotesk (UI) + IBM Plex Mono (numbers/data)
- terminal: Archivo (UI) + JetBrains Mono (everything data + buttons)
- luxe: Marcellus (display/names/buttons) + Archivo (UI) + IBM Plex Mono (numbers)
- organic: Caprasimo (display) + Figtree (everything) — loaded by `_ds/.../styles.css`

### midnight
bg `#0B0E13` · panel `#0E1219` · line `rgba(255,255,255,.07)` · text `#E7ECF3` · dim `#7C8698` · accent `#2FD98F` (on-accent `#08110C`) · danger `#F26D7E` · bet/gold `#F2C14E` · felt `radial-gradient(#154236→#0F2E27)` · plate `#141A23` · radii: buttons 8px, pills 999px, plates 10px.

### terminal
bg `#F4F2EC` · panel `#FCFBF7` · ink `#1A1712` · dim `#6D675B` · accent `#2E5FD0` · danger `#C4432F` · felt `#EBEFE6` with 1.5px ink border · all corners square (0), 1.5px ink borders, hard offset shadows `3px 3px 0 rgba(26,23,18,.14)` · dotted-grid zone background (`radial-gradient(rgba(26,23,18,.08) 1px, transparent 1px)` / 22px).

### luxe
bg `#131110` · panel `#181512` · text `#EAE3D6` · dim `#8A7D66` · gold accent `#C9A45C` / `#D9B878` (on-gold `#171310`) · muted red `#CE8080` · felt `radial-gradient(#123528→#0C271D)` with gold hairlines + deep inset shadow · plate `#1C1916` · pill radius 999px everywhere.

### organic
Token-driven from the bundled design system stylesheet (`_ds/organic-.../styles.css`): bg `var(--color-bg)` (#f5ead8) · text `var(--color-text)` (#201e1d) · accent `var(--color-accent)` (#c67139) · sage `var(--color-accent-2)` (#7a8a5e) with 100–900 ramps · felt = sage-200 fill, sage-500 2px border · plates white, radius 16px, `var(--shadow-sm)` · buttons 999px pills.

### Shared scale
Seat plate padding 6px 12px · action pill 3px 11px, 8.5px caps · pot pill 5px 16px, 12px mono bold · dock buttons 12–13px padding-block, 13–14px labels · card slots 36×50px, hero cards 48×66px · hand log 248px wide.

## Assets
None — no images. All shapes are CSS. Icons: none required (the ✕ is a text glyph). If icons are added later, Organic theme prescribes Lucide at stroke-width 2.75.

## Files
- `Poker Table.dc.html` — the interactive design reference (all four themes, desktop + mobile responsive). Contains the `themes()` token source of truth and the seat/layout geometry.
- `Mobile Preview.dc.html` — opens both mobile seat treatments side by side in iPhone frames (uses `ios-frame.jsx`). Reference only.
- `ios-frame.jsx` — iPhone bezel used by the mobile preview. **Do not integrate.**
- `support.js` — preview runtime for the reference files only. **Do not integrate.**
- `_ds/organic-049daa9e-84a1-4a65-9491-d13e353f6c31/styles.css` — the Organic design-system stylesheet (tokens + ramps) the `organic` theme reads via CSS variables.
