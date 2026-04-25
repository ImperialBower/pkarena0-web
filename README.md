# pkarena0-web

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

GPL-3.0-or-later — see [LICENSE](LICENSE).
