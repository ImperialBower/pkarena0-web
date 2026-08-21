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

### Changed
- Upgraded `pkcore` from 0.5.0 to 0.6.0, a defect-fix release. No app code
  changed: every breaking signature in 0.6.0 sits on `PokerSession::next_step`
  (new `SessionStep::Failed` arm), the fallible stud/razz constructors,
  `BettingStructure::min_raise_for_tier`, `TableAction::generate_player_loses`,
  `Shifter::shifts`, and the `TryFrom<Vec<Card>>` impls for `SevenFiveBCM` and
  `IndexCardMap` — none of which this crate calls. Shipped behaviour improves
  for free through `TableSnapshot::from_table`. The fixes that reach the table
  here are DEFECT_022 (`next_to_act` restarted its scan under the gun on every
  call, so after a re-raise the action went to a seat that had already acted on
  that bet level — pots balanced, but the order was wrong), DEFECT_023
  (`min_raise_for_tier` returned `0` for No-Limit on the first raise of a
  street, plus four public methods that always panicked now return or report),
  and `TableCelled::act_raise` no longer underflowing on a short all-in raise.
  Full suite green: 23 Rust tests, 5 bot-bundle fixtures, clippy clean, and all
  49 Playwright specs.
- Upgraded `pkcore` from 0.4.0 to 0.5.0, a TDA 2024 rules-correctness
  release. It fixes DEFECT_010 (a player who had already acted could
  re-raise a short all-in, Rule 47-A), DEFECT_011 (the odd chip in a split
  pot went to the highest-numbered winning seat instead of walking left from
  the button, Rule 20), DEFECT_012 (a short or dead blind shrank the pre-flop
  pot-limit maximum, Rule 54-B), DEFECT_013 (dead button, Rule 32) and
  DEFECT_014 (replay sized a dead-button table wrongly). Shipped behaviour
  here improves accordingly; no app code changed, because production
  snapshots are built by `TableSnapshot::from_table`, which carries the fixes
  for free. `TableSnapshot` gained public `pot_limit_pot` and `reopen_gated`
  fields, so the three test fixtures that build it as a struct literal now
  set them: `pot_limit_pot` mirrors `pot` (these are flop spots, and Rule
  54-C sizes post-flop ceilings against the real pot) and `reopen_gated` is
  `false` (the fixture seat has not acted on that street, so Rule 47-A cannot
  bar it). `PlayerAction` is now `Copy`, so the replay loop dereferences it
  instead of cloning.
- Upgraded `pkcore` from 0.3.0 to 0.4.0. That release fixes DEFECT_007, in
  which `RuleBasedDecider` emitted bets and raises the table then rejected,
  and it returned `Raise` where it had wrongly returned `Bet`. Shipped
  behaviour here improves accordingly; no app code changed. `TableSnapshot`
  gained a public `raises_this_street` field, so the three test fixtures that
  build it as a struct literal now set it to `0` — correct for these
  No-Limit flop spots, where no raise has been made and the field's only
  consumer, the Fixed-Limit raise cap, does not apply.

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
