# Design Evaluation — pkarena0-web

*Written 2026-04-25. Reflects the state of `www/index.html` and
`img/poker-table.svg` at commit `b7cd68e`.*

## Scope

The frontend lives almost entirely in two files:

- `www/index.html` (2498 lines, all CSS/JS inline)
- `img/poker-table.svg` (374 lines, embedded into the page at runtime)

The product is a single-page WASM build of NLHE vs. eight bots. It's a
self-contained PWA — manifest, dark `color-scheme` meta, icons, and an
offline voice layer in `www/audio/`. No framework, no design system, no
asset pipeline beyond `wasm-pack`.

## Verdict

**Competent functional UI with one genuinely-designed asset.** On a
frontend-design rubric (distinctive typography, atmospheric depth,
motion-as-character, memorable spatial choices), the page is roughly
**4–5 out of 10**. The SVG poker table is the only piece with a designer's
hand on it. Everything around it is default form-chrome — engineered
carefully, but not designed.

If the goal is "ship a working poker app," it succeeds. If the goal is "be
unforgettable," it doesn't yet have a visual point of view.

## What works

1. **The SVG table is real craft.** `img/poker-table.svg` lines 4–42 stack
   a radial felt gradient (`#1a6b3c → #145a30 → #0e4825`), a vertical
   wood-rim gradient (`#5c3a1e → #3e2512`), drop-shadow filters on chips
   and cards, and — the standout — a `feTurbulence` + grayscale + multiply
   blend that gives the felt a woven texture. Custom card back (navy
   diamond pattern, lines 17–20). Multi-denomination chip symbols with
   dashed inner rings. This is the one place a designer was clearly
   thinking.

2. **Semantic action-color system** (`www/index.html:107–112`). `.primary`
   (gold), `.danger` (red), `.safe` (green) recolor border + text instead
   of filling differently. At a glance, fold/call/raise read by hue, not
   by position. Communicative and constrained.

3. **Engineered responsive strategy.** Three real breakpoints, not just
   "mobile vs. desktop":
   - Portrait mobile: vertical stack.
   - Landscape mobile (`pointer: coarse and orientation: landscape`):
     two-column 58/40 split, status bar hidden to reclaim vertical space.
   - Desktop ≥768px: CSS Grid with named areas
     (`tabs / score / table+rightcol / log / yaml / foot`).

   Plus a height-aware table cap:
   `width: min(100%, 800px, calc((100vh - 240px) * 1.5))` (line 63). That
   third term is unusual and shows real problem-solving — keep the whole
   UI on one screen, derive table width from leftover height.

4. **Accessibility hygiene.** 44px minimum touch targets,
   `prefers-reduced-motion` override that flattens all animation,
   explicit `color-scheme: dark`, semantic `<details>` for the hand log.

5. **Coherent palette.** Three-hue system (deep navy `#0d0d1a`, blue
   accent `#4a90d9`, warm gold `#c8a84e` / `#f0d060`). No
   inconsistencies, no random one-off colors.

## What doesn't — the AI-default tells

1. **The font stack is the textbook generic default.** Line 23:

   ```css
   font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI',
                'Trebuchet MS', system-ui, sans-serif;
   ```

   No display face, no body pairing, no character. For a poker product
   the design space is wide open — a saloon Western face, Vegas-deco
   signage, an editorial serif treating poker as craft, even a stenciled
   tournament-broadcast feel — and the page commits to none of them.
   Georgia for card pips (line 469) is sensible but conventional. There
   is no wordmark, no logo, no display moment anywhere on the page.

2. **The palette is "casino dark"** — appropriate but generic. Navy +
   gold + forest-green felt is the default skin every poker template
   ships with. Nothing about the color choices says *this* poker app vs.
   any other. No signature accent, no surprising third color, no
   temperature shift between contexts.

3. **No motion strategy for the moments that matter.** Search result for
   `@keyframes` in `www/index.html`: one rule, `spin`, for the loading
   dot. Plus transitions on button hover (`0.15s`) and the toggle ball
   (`0.2s`). For a poker UI, the iconic visual moments — *deal*, *chip
   slide to pot*, *flop / turn / river flip*, *showdown reveal*, *winning
   hand glow*, *new hand reset* — all happen instantaneously via state
   swap. The README's "1-second delay between bot actions" is pacing,
   not motion. This is the largest design gap in the file.

4. **No page atmosphere.** `body { background: #0d0d1a }` (line 21) is a
   flat color. No gradient mesh, no vignette around the table, no grain
   overlay, no soft halo behind the felt, no smoke, no ambient texture.
   The table sits as a bright island in undifferentiated darkness.
   Compare to the SVG itself, which *does* have texture — the page
   hosting it doesn't reciprocate.

5. **Layout is stacked-and-centered.** Score bar → status → table →
   action panel → hand log → footer. No asymmetry, no overlap, no
   grid-breaking, no element that crosses a section boundary. The
   desktop grid is two columns of stacked things. Functional,
   predictable, forgettable.

6. **The chrome around the table is undifferentiated.** Score bar
   (lines 32–48), bet controls (lines 115–139), tab bar — all use the
   same `#1a1a2e` fill, `1px solid #2a2a44` border, `border-radius: 8px`
   recipe. They're correct but indistinguishable from each other and
   from any generic settings panel.

7. **`#status-msg` is the most-read element on the page** (it tells you
   what just happened, whose turn it is, what the bot did) and it's
   styled `font-size: 13px; color: #88aacc` (lines 51–57). Mid-blue,
   small, on navy. For the live narration of a poker hand, that's the
   wrong amount of weight.

## The single thing worth remembering

The felt. Specifically the procedural-noise filter on the felt
(`#feltNoise`, SVG lines 35–42). It's a quiet detail you don't notice
until you compare it to a flat-color table elsewhere — and then it's the
reason this table reads as a *table* and not as a green ellipse. Outside
the SVG `viewBox`, nothing else competes for that slot.

## Where redesign would pay off

Ranked by leverage-to-effort.

1. **Pick a real type pairing** (highest leverage, lowest effort).
   Replace the generic stack with one display face and one body face.
   Directions worth considering:
   - *Tournament-broadcast*: a tight condensed sans (Saol Display, GT
     America Condensed) for numbers/headlines + a humanist sans for
     body.
   - *Saloon / Western*: a slab serif (Playfair Display, Domaine
     Display) for the title and seat labels, with a clean grotesk for
     chrome.
   - *Editorial poker-as-craft*: a serious serif (Söhne Mono for chips,
     New York / IBM Plex Serif for text) — treats the table like
     The Athletic, not a casino.

   The page is a single static HTML file, so this is one `<link>` to a
   font CDN plus one CSS variable rename.

2. **Animate the moments**, not the chrome. CSS-only is fine.
   - Cards deal: `transform: translate(...)` from the dealer position +
     `rotate(-3deg)` settle, with `animation-delay` staggered by seat.
     ~800ms total.
   - Chips to pot: a single CSS keyframe sliding chip stacks toward the
     pot center on bet confirm.
   - Board reveal: `transform: rotateY(180deg)` flip on flop/turn/river,
     with `transition-delay` cascading per card.
   - Winner glow: a brief `box-shadow` pulse around the winning seat
     plate at showdown.

   `prefers-reduced-motion` is already respected, so any of these
   degrades cleanly.

3. **Give the page atmosphere.** Replace the flat body background with
   either (a) a radial gradient halo behind the table
   (`radial-gradient at 50% 40%, #161628 0%, #0d0d1a 60%`), (b) a faint
   SVG grain overlay at ~3% opacity, or (c) a single ambient vignette at
   the edges. Pick one, not three.

4. **Promote `#status-msg`.** This is the live commentary. Up the size
   (16–18px), give it character (italic? small caps? the display face?),
   and let it live just above the table with a tiny entrance animation
   when the message changes. It's the most narratively-loaded element in
   the UI; right now it reads as a footnote.

5. **Differentiate the chrome.** Score bar, bet controls, and tab bar
   currently use the same recipe. Score bar should feel like a heads-up
   display (thin top rule, no rounded corners, monospace numbers). Bet
   controls should feel tactile (slightly inset, larger). They serve
   different jobs; they should look different.

6. **A wordmark / brand moment.** The product is called pkarena0. Right
   now the only place that name appears is the browser tab. A small
   fixed wordmark in a corner — set in whatever display face you pick —
   costs nothing and gives the product an identity.

## Files referenced

- `www/index.html:23` — the font stack.
- `www/index.html:51–57` — `#status-msg`, the underweighted element.
- `www/index.html:93–112` — the action button system (preserve).
- `www/index.html:489–700` — responsive breakpoints (preserve).
- `img/poker-table.svg:35–42` — the felt-noise filter (the lone designed
  moment).

## Reproducing this evaluation

```
make build       # build WASM into www/pkg
python3 -m http.server 8080 --directory www
open http://localhost:8080
```
