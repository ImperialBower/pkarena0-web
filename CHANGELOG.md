# Changelog

All notable changes to this project will be documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Lifetime P&L tracker in the Play-tab score bar. Persists across reloads
  via `localStorage`; commits each completed game's net delta on
  `SessionOver` or **New Game**. Reset button in Settings (with
  confirmation). Designed in `docs/FEATURE_pnl.md`.
- Score bar now shows hand and blinds together as `Hand: N (sb/bb)`,
  reclaiming horizontal space for the new P&L slot.

### Removed
- Score-bar P&L indicator (Play mode). It duplicated information already
  visible in the chip count and had no role in a single-session,
  no-stakes app.

## [0.1.0] — 2026-04-25

First tagged 0.1.x release. Summarizes everything shipped during the 0.0.x
prototype phase, plus the cleanup that unblocked the bump.

### Added
- Single-player No-Limit Hold'em against eight bots, served as a static
  WASM page on GitHub Pages.
- Arena (all-bot) tab alongside the human Play tab; tab switching halts the
  loop in the inactive tab cleanly (#3, #4).
- Settings gear with persisted preferences; sound toggle defaults to off
  (#4, #5).
- Phase 1 audio layer: voice clip stitcher with `SpeechSynthesis` TTS
  fallback, `LiveAdapter` polling `get_state()` and emitting `GameEvent`s,
  voice narration toggle, "Test voice" button (#5).
- Per-suit card colouring in the SVG table (#8).
- Hand log shows every player's hole cards (#7).
- `version()` WASM export; score bar links the displayed version back to
  the GitHub repo.
- YAML hand-history export via the **Export YAML** button.
- Playwright end-to-end test suite (game, UI, YAML download) running on
  every PR via GitHub Actions.
- `CHANGELOG.md` (this file).
- README "Audio (experimental)" section pointing at the Phase 1 docs.
- `Cargo.lock` is now tracked, pinning the dependency graph for reproducible
  release builds and stable CI cache keys.

### Changed
- Bot pool upgraded for stronger play (#6).
- Arena score bar shows only the hand number + settings gear (chips/P&L
  hidden in all-bot mode).
- `pkcore` bumped iteratively from `0.0.43` → `0.0.50`, picking up betting
  fixes, leak fixes, and bot improvements along the way.

### Fixed
- Three-way pot betting bug where a `call` followed by a re-raise blocked
  the player.
- Even-split bug after a player busts out.
- Tab-switching could leave a stale animation loop running.
- Blinds wiring during the human Play loop.
- Chrome `SpeechSynthesis.cancel()` regression: removed the cancel call
  from `voice.cancel()` and the test handler; added a 50 ms TTS debounce.
- `cargo check`/`cargo build` no longer fail on a missing `validate-yaml`
  binary stanza in `Cargo.toml`. The corresponding `make test-yaml` target
  has been removed until the validator is reinstated.
- README now lists the same minimum Rust toolchain as `Cargo.toml`
  (`1.94.1`).
