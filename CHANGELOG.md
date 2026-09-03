# Changelog

All notable changes to this project will be documented in this file.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.28] — 2026-09-02

### Changed
- Upgraded `pkcore` from 0.12.0 to 0.12.1, a single-defect release. **No app
  code changed**, and no new knob or cargo feature ships in it — but the defect
  it fixes lands squarely on the strong tier, so the bump alone is the whole
  benefit.

  `preflop_charts: solver` built its equity request with
  `EquityOptions::default()`, whose `max_samples` is **25,000**, instead of
  reading the profile's `equity` knob. `strong_decision()` asks for
  `EquityMode::Fast { samples: 500 }` (`src/lib.rs:2259-2266`) and
  `data/bots/strong.yaml` carries the same pair, so every strong-tier bot was
  spending **50x its stated budget** — on preflop, the most frequent decision in
  a hand, and the one street with no board to narrow the runouts. The knob now
  governs both streets alike: `fast { samples }` spends that many, `exact`
  spends 100,000, and `off` — legal here, since `solver` runs without the engine
  knob — spends pkcore's own default of 2,000. See pkcore EPIC-39 corrigendum 18.

  The docstring at `src/lib.rs:2256-2257` claimed `Solver` "reuses the same
  `compute()` the `equity` knob already runs, so preflop costs one sampled call
  rather than a coin flip". That was true of the *call* but not of its *budget*;
  as of 0.12.1 it is true of both, so the comment stands unamended.

  **Standard and weak are unaffected.** Standard sets `preflop_charts: off`
  (`standard_decision()`, `src/lib.rs:2238-2244`) and weak carries no `decision:`
  block at all, so neither ever entered the solver path.

- No feature flags were added or changed. The four-feature pin
  (`bot-profiles`, `hand-histories`, `equity`, `player-stats`) still covers
  everything this crate calls. The 0.12.0 arrivals stay deliberately out:
  `hup-charts` links `generated/hups.bin` — 15.8 MB, which took the WASM
  download from 478 KB to 3.84 MB brotli when it was reachable by accident — and
  `preflop_charts: hup` is heads-up only at a six-handed table; `parallel` links
  a rayon thread pool a browser cannot run; `exploit` stays a user toggle rather
  than a tier lever (EPIC-49 corrigendum §1); and `store`, `terminal`,
  `bot-training`, `generators`, `pokerbench`, `player-stats-persistence` and
  `debug-json` are host-side tooling with no browser surface.

  One trap the fix creates, recorded so it is not walked into: `preflop_charts:
  solver` on the **standard** tier would now cost **2,000** samples per preflop
  decision, because standard's `equity` is `off` and `off` falls back to
  pkcore's default. That is 4x the strong tier's 500 — standard would decide
  slower than strong. Adopting it there means pinning an explicit
  `equity: fast { samples }` first.

  Verified: 30 Rust tests pass, 5 bot-bundle parity fixtures pass
  (`make validate-bots`), `make build` is clean, and
  `tests/strong-equity-latency.spec.ts` passes. WASM download is **472 KB**
  brotli (2.1 MB raw) — no growth.

## [0.1.27] — 2026-08-30

### Changed
- Upgraded `pkcore` from 0.7.0 to 0.11.0 (four releases). **No app code
  changed** — the crate compiles, clippy-clean, on both the host and
  `wasm32-unknown-unknown`, and all 30 Rust tests plus all 65 Playwright specs
  pass untouched. Every breaking change in the range lands on a surface this
  crate does not call:
  - 0.8.0 removed the whole `TableCelled` family and re-based `Dealer` on
    `casino::table::Table`; `TestData` fixtures now return plain types. This
    crate drives `PokerSession` directly and owns its own fixtures, so none of
    it reaches here.
  - 0.10.0 added Pluribus-format hand *export*; not used here.
  - 0.11.0 dropped `store` and `terminal` from the default feature set — a
    no-op for us, since `Cargo.toml` has always pinned
    `default-features = false` with an explicit four-feature list
    (`bot-profiles`, `hand-histories`, `equity`, `player-stats`), all of which
    still exist. The combinatorics signatures (`Pile::combinations_*`,
    `Cards::combinations`, `Deck::combinations`, `Outs::iter`) now return
    `impl Iterator`, and `FIVE_CARD_COMBOS` / `Deck::to_par_iter` were removed;
    this crate calls none of them. `TableManager` and `TableEvent` are
    `#[deprecated]`; not used here, so no new warnings.
  - Two silent 0.11.0 behaviour changes were checked rather than assumed.
    `Card` deserialization now errors on an index it cannot parse instead of
    yielding a blank card — this crate never deserializes cards from an
    untrusted payload. `EquityOptions::max_samples` dropped its default from
    100,000 to 25,000, which would silently coarsen equity for anyone riding
    the default; the strong tier sets `EquityMode::Fast { samples: 500 }`
    explicitly and `pkcore::bot::decider::real_equity` passes that straight
    into `max_samples`, so bot decisions are unaffected.
- The dependency tree lost **16 crates** and no crate was added
  (`Cargo.lock` shrank by 166 lines). `rayon` and `rayon-core` leave because
  0.11.0 moved every rayon entry point behind a new default-on `parallel`
  feature that a `default-features = false` consumer never enables — 0.11.0's
  release notes name this browser build as the reason the gate exists, since
  the old `analysis::equity::compute` linked a thread pool into wasm32 that a
  browser can never run. `pkstate` leaving pkcore took `chrono` with it, and
  `chrono` took its platform tail (`iana-time-zone`,
  `android_system_properties`, `core-foundation-sys`, five `windows-*` crates)
  plus the `cc` / `shlex` / `find-msvc-tools` build chain.
- Dropped a stale `[[patch.unused]] pkcore 0.11.0` stanza that a removed local
  patch had left behind in `Cargo.lock`.

## [0.1.25] — 2026-08-22

### Added
- Lifetime P&L tracker in the Play-tab score bar. Persists across reloads
  via `localStorage`; commits each completed game's net delta on
  `SessionOver` or **New Game**. Reset button in Settings (with
  confirmation). Designed in `docs/FEATURE_pnl.md`.
- Score bar now shows hand and blinds together as `Hand: N (sb/bb)`,
  reclaiming horizontal space for the new P&L slot.

### Changed
- Upgraded `pkcore` from 0.6.0 to 0.7.0. One breaking signature reaches this
  crate: `PokerSession::next_actor` now returns
  `Result<Option<u8>, PKError>`. Before 0.7.0 a failed street advance (a dry
  deck) collapsed to `None`, which `step_bot` read as "hand over" — the phase
  went to `HandComplete`, `end_hand()` then returned `ActionIsntFinished`, and
  the pot was stranded. `step_bot` now unwinds an `Err` with
  `PokerSession::abort_hand` (every committed chip goes back to its owner),
  records it in `LAST_ERROR`, and returns `{"done":true,"error":"…"}`. The
  other 0.7.0 breakages (`KuhnCfr::train`, `Deck::get`,
  `Terminal::receive_usize`, `HUPResult::from_sorted_heads_up`) and the removed
  `pkcore::play::actions` / `pkcore::play::positions` modules are not used
  here. Six test call sites gained `.expect("next_actor")`.
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
