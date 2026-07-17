# EPIC-49: Data-Driven Bot Lineup & Difficulty

> **One-line:** Move the bot lineup from hardcoded `default_profiles()` to
> YAML-defined profiles, give every archetype position awareness, and surface
> a difficulty selector that maps to graded capability bundles — with arena
> mode as the built-in chips/100 measurement harness.

## Status

**CLOSED 2026-07-16** (branch `EPIC-46`). All three phases landed; the one
upstream-shaped row (EPIC-36 `decision:` knobs) is deferred to the EPIC-48
follow-up, with the bundles already structured to receive it.

| Component | Status |
|---|---|
| Load `BotProfile` YAML (embedded `data/bots/standard.yaml`) instead of only `default_profiles()` | **Complete** (`standard_profiles()`, `src/lib.rs`) |
| YAML validation + code-parity gate (`make validate-bots`) | **Complete** (`bot_bundle_fixture` tests; Makefile hook; now covers all three bundles) |
| Position awareness: `Playbook` attached to all archetypes | **Complete** (`attach_archetype_playbook`, `src/lib.rs`; seeded BTN≠UTG test per archetype) |
| Difficulty selector UI → profile bundle (weak / standard / strong) | **Complete** (`#difficulty-select` in Settings; `set_difficulty` / `get_state().difficulty`) |
| Difficulty honesty: chips/100 ordering weak < standard < strong | **Complete** (`difficulty_ordering_tests`; `make bench-tiers`) |
| Strong bundle consumes EPIC-36 `decision:` knobs | **Deferred** (upstream EPIC-36 still Planned; interim strong = `strengthen()` — see corrigendum) |
| Arena mode chips/100 report (exportable) | **Complete** (`session_report` in `get_state()`; arena hand-log leaderboard at session end) |

**Depends on:** [EPIC-46](EPIC-46_Decider_Integration.md).
**Consumes:** [EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) (adaptation as
a difficulty lever), [EPIC-48](EPIC-48_Real_Equity_WASM.md) / upstream EPIC-36
(capability knobs as difficulty levers).

---

## Context

The lineup today is fixed in code: `BotProfile::default_profiles()` + `joker`
(`src/lib.rs:93-96,146-149`), shuffled per session. Findings from the
capability audit (2026-07-12):

- **Position awareness is patchy.** Only `gto`, `tight_passive`, and
  `loose_aggressive` carry a `Playbook` (pkcore `src/bot/profile.rs:292,314,336`);
  the other five play identically from UTG and the button. pkcore already has
  `PositionRanges::gto_six_max/nine_max` etc.
  (`src/bot/position_ranges.rs:193-412`) and profile-level resolution
  (`profile.rs:738,797`) — the gap is data, not code.
- **Dead range fields.** `three_bet` / `call_three_bet` exist on every
  `RangeStrategy` (pkcore `src/bot/range_strategy.rs:40-42`) but the decider
  never consults them (upstream EPIC-36 territory; YAML profiles should carry
  them so they light up when upstream wires them).
- **YAML support is already paid for.** The `bot-profiles` feature (enabled
  since day one) provides `BotProfile::from_yaml_str/to_yaml_string`
  (`profile.rs:826-899`) — the web app just never uses it. pkcore ships
  example profiles under `data/bots/`.
- **No difficulty concept.** Personality is the only axis; a beginner and a
  grinder face the same table. Upstream EPIC-36's whole premise is bots of
  *tunable strength* from YAML — the web app is its most natural consumer.
- **Arena mode is an untapped bench.** All-bot arena mode (`IS_ALL_BOT`,
  `src/lib.rs:64,142`) plus seeded RNG is 90% of a chips/100 harness — the
  browser equivalent of EPIC-36's `SimTable` bench, and a way to *validate*
  that a "strong" bundle actually beats a "weak" one before shipping either.

## Goals

- Bot lineups defined by embedded YAML, selectable at session start.
- Every archetype position-aware via `Playbook`.
- A 3-tier difficulty selector whose tiers are honest — validated by seeded
  arena chips/100, not vibes.
- Profile YAML forward-compatible with EPIC-36 `decision:` sections.

## Scope

**In scope:** embedded YAML loading (`include_str!`; no fetch/CORS concerns),
playbook data for all archetypes (upstreamable to pkcore `data/bots/`),
difficulty selector UI + plumbing, arena chips/100 report, YAML validation
tooling.

**Out of scope:** user-editable profiles in the browser (later);
per-seat difficulty mixing (later); any decider logic changes.

---

## Design

### Embedded YAML lineup

```rust
static BUNDLE_STANDARD: &str = include_str!("../data/bots/standard.yaml");
// parse Vec<BotProfile> at session start; on parse error, console warn +
// fall back to default_profiles() so the game never bricks.
```

Bundles: `weak.yaml`, `standard.yaml` (≈ today's eight archetypes + playbooks
for all), `strong.yaml` (EPIC-36 knobs on, adaptation on). Until upstream
EPIC-36 ships, `strong` = standard + EPIC-47 adaptation.

### Difficulty selector

Setup screen control (matching existing setup UI patterns) → picks the
bundle + sets the EPIC-47 adaptivity toggle. Persist choice in localStorage.

### Arena chips/100 report

Arena mode already runs seeded all-bot sessions. Add to `get_state()` /
session-end JSON: per-seat `net_chips`, `hands_played`, computed chips/100.
A Playwright spec (Turbo, fixed seed, N hands) asserts the strong bundle
outperforms the weak bundle — the acceptance gate for difficulty honesty.
Note tournament-style elimination biases chips/100 (upstream EPIC-36 flags
the same issue and plans a cash-mode reset in `SimTable`); for the web
harness, either restart-on-bust with fixed stacks (arena mode already
handles elimination) or aggregate multiple short seeded runs.

### Validation tooling

`src/bin/validate_yaml.rs` currently validates `HandCollection` YAML only.
Extend (or add `validate-bots`) to parse every `data/bots/*.yaml` via
`BotProfile::from_yaml_str`, run in CI/Makefile so a bad profile can't ship.

---

## Work Items

### Phase 1 — YAML lineup ✅
- [x] 1a. `data/bots/standard.yaml` captures today's nine profiles
  (8 `default_profiles()` + `joker`), generated by the `#[ignore]`d
  `generate_standard_bundle` fixture test via `serde_yaml_bw`. Format is a
  named wrapper `{ name, profiles: [...] }` (`BotBundle`) for future tier
  metadata. Regenerate: `cargo test --lib generate_standard_bundle -- --ignored`.
- [x] 1b. `include_str!` + `serde_yaml_bw` parse + fallback in
  `standard_profiles()`; both `init_game` / `init_bot_game` now shuffle the
  embedded pool. On parse failure it `console_warn`s and returns the built-in
  pool so a bad edit can't brick the game.
- [x] 1c. Validation is a `cargo test` gate (`bot_bundle_fixture` module), not a
  bin — this crate is `cdylib`-only. `make validate-bots` runs it and `make
  test` now depends on it, so an unparseable/drifted profile fails the build.
- [x] 1d. `standard_bundle_matches_default_pool`: the embedded YAML round-trips
  to *exactly* `default_profiles()` + `joker()` (`BotProfile: Eq` compares every
  range/betting/playbook field) — lineup behavior is provably unchanged.

### Phase 2 — Position awareness for all ✅
- [x] 2a. Playbooks authored for the five flat archetypes
  (`attach_archetype_playbook`, `src/lib.rs`): 6-max + 9-max
  `PositionalBetting` graded around each archetype's flat baseline, paired
  with the closest existing pkcore `PositionRanges` chart. Bonus finding:
  pkcore's own `tight_passive` (both sizes) and `loose_aggressive` (9-max)
  playbooks were positionally *flat* — re-graded here too, since this app
  deals 9-max. Upstreaming to pkcore `data/bots/` remains open (tracked in
  Open Questions).
- [x] 2b. `three_bet`/`call_three_bet` verified populated on every profile
  (`every_profile_carries_three_bet_ranges`); `short_stack_ninja`'s empty
  `call_three_bet` is intentional upstream (push-or-fold never flat-calls —
  pkcore has a test locking it).
- [x] 2c. `btn_and_utg_decisions_diverge_for_each_archetype`: seeded,
  deal-independent (authored snapshots) — BTN and UTG action streams differ
  for all seven graded archetypes.

### Phase 3 — Difficulty tiers ✅
- [x] 3a. `weak.yaml` (`weaken()`: spewy over-bluffer, no value extraction,
  position-blind) and `strong.yaml` (`strengthen()`: tight ~10% range,
  bluff clamp, value threshold, position grades kept). Both generated from
  code pools with parity gates, same as `standard.yaml`. Design deltas from
  the original sketch are measured, not stylistic — see corrigendum.
- [x] 3b. Settings-overlay selector (`#difficulty-select`) + localStorage
  (`difficulty` key) + `set_difficulty()` into both WASM instances at boot
  and on change; `get_state().difficulty` surfaces the engine's live value
  (the app has no setup screen — Settings is where the adaptive toggle
  already lives).
- [x] 3c. `session_report` (per-seat net chips, hands, chips/100) in
  `get_state()`; arena hand-log leaderboard at session end; ordering gate
  landed as the **native** matchup bench `difficulty_ordering_tests`
  (`make bench-tiers`) rather than a Playwright spec, and it is
  **statistical, not fixed-seed** — pkcore has no seeded deck
  (`start_hand` shuffles from the entropy RNG), so no browser or native
  harness can replay identical deals. Playwright covers the plumbing
  (`tests/difficulty.spec.ts`: persistence, engine lockstep, weak = 8-seat
  arena, report sanity).

---

## Key Files

| File | Role |
|---|---|
| `data/bots/{weak,standard,strong}.yaml` | lineup bundles (generated; parity-gated) |
| `src/lib.rs` `standard_profiles()` / `weak_profiles()` / `strong_profiles()` / `BotBundle` | YAML pool construction |
| `src/lib.rs` `attach_archetype_playbook` / `weaken` / `strengthen` | tier transforms (source of truth for the bundles) |
| `src/lib.rs` `difficulty_ordering_tests` | chips/100 matchup bench |
| `bot_bundle_fixture` tests + `make validate-bots` | validation (no bin — `cdylib` crate) |
| `www/index.html` + `www/js/main.js` Settings overlay | difficulty selector |
| `tests/difficulty.spec.ts` | selector persistence + engine lockstep + report sanity |
| pkcore `src/bot/{playbook,position_ranges,positional_betting}.rs` | position data types |
| pkcore `src/bot/profile.rs:826-899` | YAML round-trip |
| `Makefile` | `validate-bots` + `bench-tiers` hooks |

---

## Verification

```bash
make validate-bots        # all three bundles parse + match their code pools
cargo test                # position tests, plumbing, report invariants (fast)
make bench-tiers          # chips/100 ordering weak < standard < strong (~15s, release)
make build && npx playwright test   # selector persistence + report plumbing
```

Acceptance at close: (1) standard bundle is provably generated from the code
pool (parity test) — behavior-identical *until Phase 2 deliberately changed
it* by design; (2) every archetype plays position-differentiated poker
(seeded BTN≠UTG test); (3) chips/100 ordering weak < standard < strong holds
with statistical margin (≈5σ / ≈12σ — fixed-seed reproduction is impossible,
see corrigendum §3); (4) invalid YAML cannot ship (`make validate-bots`) and
cannot brick a session (runtime fallback per bundle).

---

## Implementation corrigendum (2026-07-16, branch `EPIC-46`)

All three phases shipped. The deltas below are measured decisions, each
carrying its evidence.

1. **Strong ≠ standard + adaptation.** The original sketch made forced
   EPIC-47 adaptation the strong tier's interim lever. The matchup bench
   falsified it: adaptive-wrapped standard profiles vs bare ones measured a
   consistent mild *drag* (−2.7k and −3.8k chips/100 over two 96k-hand
   runs). Adaptation's value proposition is modeling *human* tendencies,
   which a bot-vs-bot bench cannot see — so it stays a user toggle
   (honored on standard and strong, forced off on weak), and the strong
   lever is the `strengthen()` bundle instead: tight ~10% opening range,
   bluff frequency clamped ≤8, `value_threshold: 0.5`, position grades
   kept. Measured: **+24k chips/100 over standard** (σ ≈ 2k across three
   96k-hand runs).
2. **Weak ≠ loose-passive.** Two candidate weakenings were measured and
   rejected: an "any two cards" fish gambles too much (all-in variance
   drowns the signal, and it doesn't reliably lose), and an over-tight nit
   actually *profits* against these over-bluffing archetypes (+5k..+23k
   chips/100 in every run — "tight is right"). The weak form that loses
   reliably is the spewy over-bluffer with no value extraction
   (`weaken()`: bluff 40, aggression 5, `value_threshold: 0.97`,
   half-pot only, playbook stripped). Measured: **−22k chips/100 vs
   standard** (σ ≈ 4k across four 12k-hand runs).
3. **The ordering gate is statistical, not fixed-seed.** pkcore's
   `start_hand` shuffles from the entropy thread-local RNG — there is no
   seeded deck — so "reproduces under a fixed seed" (original acceptance
   #3) is unimplementable in any harness, browser or native. The gate is
   the native bench `difficulty_ordering_tests` (`make bench-tiers`,
   release, ~15s), sized to ≈5σ (weak, 12k hands) and ≈12σ (strong, 96k
   hands). It is `#[ignore]`d out of the default fast suite because it is
   entropy-dealt by construction.
4. **The bench needed a cash-mode reset.** A restart-on-bust variant let
   winners' stacks grow without bound and the game's character drifted
   with depth — the same matchup produced opposite signs at 12k vs 96k
   hands. Fixed-stack reset after every hand (bank the result, restore
   100 BB) made the process stationary; this independently validates the
   cash-mode-reset plan in upstream EPIC-36's `SimTable` bench.
5. **Playbook re-grades went further than planned.** Phase 2 scoped five
   flat archetypes; pkcore's own `tight_passive` (6-max and 9-max) and
   `loose_aggressive` (9-max) playbooks turned out to be positionally flat
   too, and this app deals 9-max — so seven archetypes got graded
   positional betting (`attach_archetype_playbook`), leaving only `gto`
   (already graded upstream) untouched.
6. **No joker in the weak or strong bundles.** `JokerDecider` ignores its
   profile and morphs into full-strength `default_profiles()` each hand,
   which would leak standard play into either tier. Consequence: weak and
   strong arenas seat 8 bots, standard seats 9 (asserted in
   `tests/difficulty.spec.ts`).
7. **Selector lives in the Settings overlay**, not a "setup screen" — the
   app has none; Settings is where the EPIC-47 adaptive toggle already
   lives, and the selector follows its exact lifecycle (localStorage,
   push to both WASM instances, applies on next New Game / Start Arena,
   `get_state().difficulty` for engine/UI lockstep).
8. **Inherited debt / handoffs:** upstreaming the seven playbooks to
   pkcore `data/bots/` remains open; the EPIC-36 `decision:` knobs land in
   these bundles when the EPIC-48 follow-up opens; `three_bet`/
   `call_three_bet` stay dormant until upstream wires them.

| Phase | Status at close |
|---|---|
| Phase 1 — YAML lineup | **Complete** |
| Phase 2 — position awareness for all | **Complete** |
| Phase 3 — difficulty tiers | **Complete** (EPIC-36 knob adoption deferred to the EPIC-48 follow-up) |

---

## Open Questions

- **Tier naming & count.** Three tiers, or a fourth "adaptive" tier that
  isolates EPIC-47 as its own lever?
- **Upstreaming playbook data.** Land the five new playbooks in pkcore's
  `data/bots/` (benefits pkdealer too) or keep them web-local? Prefer
  upstream.
- **Human-visible strength labels.** Show per-bot style names only, or also
  a strength hint? (Hiding strength preserves the "read your opponents"
  experience.)
