# pkarena0-web

[![Deploy to GitHub Pages](https://github.com/ImperialBower/pkarena0-web/actions/workflows/deploy.yml/badge.svg)](https://github.com/ImperialBower/pkarena0-web/actions/workflows/deploy.yml)
[![Playwright Tests](https://github.com/ImperialBower/pkarena0-web/actions/workflows/playwright.yml/badge.svg)](https://github.com/ImperialBower/pkarena0-web/actions/workflows/playwright.yml)
[![Release embed](https://github.com/ImperialBower/pkarena0-web/actions/workflows/release.yml/badge.svg)](https://github.com/ImperialBower/pkarena0-web/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE-MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE-APACHE)

A browser-based, single-player No-Limit Texas Hold'em game where one human
faces a table of eight AI bots. The game engine is written in Rust, compiled
to WebAssembly via [wasm-pack](https://rustwasm.github.io/wasm-pack/), and
served as a single static HTML page — no server required.

**Live demo:** <https://imperialbower.github.io/pkarena0-web/>

---

## How it works

| Layer | Technology | Role |
|---|---|---|
| Game engine | Rust (`pkcore` crate) | Hand logic, bot AI, hand histories |
| WASM bindings | `wasm-bindgen` | Exposes Rust functions to JavaScript |
| Frontend | Vanilla HTML/CSS/JS | SVG poker table, action panel, hand log |
| CI/CD | GitHub Actions | Build WASM → deploy to GitHub Pages |

The entire game state lives in Rust `thread_local!` singletons. JavaScript
calls a small set of exported functions (`init_game`, `step_bot`,
`human_action`, `next_hand`, `get_state`, `get_session_yaml`) and receives
JSON back — no state is passed back to Rust from the browser.

---

## Prerequisites

- [Rust](https://rustup.rs/) (stable, ≥ 1.94.1) with the `wasm32-unknown-unknown` target:

  ```
  rustup target add wasm32-unknown-unknown
  ```

- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/):

  ```
  curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
  ```

---

## Building

```bash
# Debug build (fast compile, larger WASM)
wasm-pack build --target web --out-dir www/pkg

# Release build (optimised, smaller WASM — used by CI)
wasm-pack build --release --target web --out-dir www/pkg
```

The output lands in `www/pkg/`. Open `www/index.html` in a browser (you need
a local HTTP server because WASM requires HTTPS or `localhost`):

```bash
# Python 3
python3 -m http.server 8080 --directory www
# then open http://localhost:8080
```

---

## Running tests

End-to-end tests use [Playwright](https://playwright.dev/) against the built
WASM.

```bash
# Install Node dependencies once
npm install
npx playwright install --with-deps chromium

# Build WASM first, then run tests
wasm-pack build --target web --out-dir www/pkg
npx playwright test

# Interactive UI mode
npx playwright test --ui
```

---

## Gameplay

- You sit at **seat 0** with $10,000 in chips; eight bots fill the remaining seats.
- Blinds are fixed at **$50 / $100**.
- The bots are randomly drawn from a pool of profiles defined in `pkcore` and
  play with varying aggression styles.
- Bot actions are animated one at a time (1-second delay each) so you can
  follow the action.
- At showdown, bot hole cards are revealed.
- After a hand ends, click **Next Hand** (or any action button) to deal again.
- The session ends when fewer than two players have chips.
- Use the **Export YAML** button to download a hand history of the entire
  session.

---

## Embedding the table in your own page

Every tagged release carries `pk-table-<tag>.tar.gz`: one flat directory
with a custom element, its stylesheet, the engine, and the licenses. Unpack
it beside your page and write two lines:

```html
<script type="module" src="pk-table/pk-table.js"></script>
<pk-table seed="42"><img src="table.png" alt="A poker table"></pk-table>
```

The `<img>` is what readers see with script off, from a file, or on paper;
the element adds a **Deal** button under it and fetches nothing until Deal is
pressed. Then it loads the engine, swaps the picture for the live table, and
plays one hand at a time against eight bots. If the engine will not load, the
picture stays and the status line says so.

| Attribute | Meaning | Default |
|---|---|---|
| `seed` | Seeds which bots sit down (the cards are dealt fresh every hand). | fresh per Deal |
| `delay` | Milliseconds between bot actions. | `1000` |
| `base` | Where `pkarena0_web.js` and `pkarena0_web_bg.wasm` live. | beside `pk-table.js` |
| `css` | Where `pk-table.css` lives. | beside `pk-table.js` |
| `bots` | **Reserved** for [EPIC-54](docs/EPIC-54_Player_Mode_Table_Setup.md); ignored with a console warning. | `8` |

Events (they bubble): `pk-table:ready` (`detail.version`),
`pk-table:hand-over` (`detail.state`, the engine's state after the hand
settles), `pk-table:error` (`detail.message`). Each `<pk-table>` on a page is
its own game. Every request goes to the directory the module came from — no
fonts, no CDN. The surround follows your page's `--bg`, `--fg` and `--links`
custom properties when present; the felt stays green.

`www/embed.html` is a working example (`make serve`, then
<http://localhost:8080/embed.html>), and `make embed` builds the directory
locally into `dist/pk-table/`. `MANIFEST.sha256` lists every file's digest;
`sha256sum -c MANIFEST.sha256` verifies an unpacked copy. The rules the
element keeps are tested in `tests/embed.spec.ts` and written up in
[EPIC-56](docs/EPIC-56_Embed_Build.md).

## Audio (experimental)

A Phase 1 audio layer ships in `www/audio/` that narrates each game action.
The voice toggle in the settings panel falls back to the browser's
`SpeechSynthesis` engine — pre-recorded clips under `www/audio/voice/` are
not yet bundled. See [`docs/audio-integration.md`](docs/audio-integration.md)
and [`docs/voice-script.md`](docs/voice-script.md) for the design and the
clip script.

---

## Dependencies

| Crate | Purpose |
|---|---|
| [`pkcore`](https://crates.io/crates/pkcore) | NLHE engine, bot profiles, hand histories |
| `wasm-bindgen` | Rust ↔ JavaScript bridge |
| `serde` / `serde_json` | JSON serialisation of game state |
| `rand` | Seeded RNG for reproducible shuffles |
| `console_error_panic_hook` | Rust panics surfaced in the browser console |

---

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
