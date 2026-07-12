# EPIC-49: Data-Driven Bot Lineup & Difficulty

> **One-line:** Move the bot lineup from hardcoded `default_profiles()` to
> YAML-defined profiles, give every archetype position awareness, and surface
> a difficulty selector that maps to graded capability bundles — with arena
> mode as the built-in chips/100 measurement harness.

## Status

| Component | Status |
|---|---|
| Load `BotProfile` YAML (embedded `data/bots/*.yaml`) instead of only `default_profiles()` | Planned |
| Position awareness: `Playbook` attached to all archetypes (today 3 of 8) | Planned |
| Difficulty selector UI → profile bundle (weak / standard / strong) | Planned |
| Strong bundle consumes EPIC-36 `decision:` knobs + EPIC-47 adaptation | Planned (post upstream EPIC-36) |
| Arena mode chips/100 report (seeded, exportable) | Planned |
| `validate-yaml` bin extended to validate `BotProfile` YAML | Planned |

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

### Phase 1 — YAML lineup
- [ ] 1a. Create `data/bots/standard.yaml` capturing today's nine profiles
  (export via `to_yaml_string` from a small native bin/test).
- [ ] 1b. `include_str!` + parse + fallback; replace hardcoded pool
  construction (`src/lib.rs:93-96,146-149`).
- [ ] 1c. Extend YAML validation bin + Makefile hook.
- [ ] 1d. Seeded regression: standard.yaml lineup ≡ default_profiles()
  behavior.

### Phase 2 — Position awareness for all
- [ ] 2a. Author `Playbook`/`PositionRanges` data for the five flat
  archetypes (tight_aggressive, loose_passive, maniac, abc,
  short_stack_ninja) — offer upstream to pkcore `data/bots/`.
- [ ] 2b. Populate `three_bet`/`call_three_bet` fields (dormant until
  upstream wires them; harmless now, ready later).
- [ ] 2c. Test: BTN vs UTG decisions differ for each archetype (seeded).

### Phase 3 — Difficulty tiers
- [ ] 3a. `weak.yaml` (loosened ranges, low aggression discipline; post
  EPIC-36: knobs all-off) and `strong.yaml` (playbooks + EPIC-47 adaptation;
  post EPIC-36: `equity: fast`, `outs: on`, `pot_odds` strict).
- [ ] 3b. Setup-screen selector + localStorage persistence.
- [ ] 3c. Arena chips/100 in session-end state; Playwright ordering spec
  (weak < standard < strong, fixed seed, Turbo).

---

## Key Files

| File | Role |
|---|---|
| `data/bots/{weak,standard,strong}.yaml` (new) | lineup bundles |
| `src/lib.rs:93-96,146-149` | pool construction → YAML |
| `src/bin/validate_yaml.rs` | extend for `BotProfile` validation |
| `www/` setup screen | difficulty selector |
| pkcore `src/bot/{playbook,position_ranges}.rs` | position data types |
| pkcore `src/bot/profile.rs:826-899` | YAML round-trip |
| `Makefile` | validation hook |

---

## Verification

```bash
cargo run --bin validate-yaml -- data/bots/standard.yaml   # (extended form)
cargo test                                                  # seeded equivalence + position tests
make build && npx playwright test                           # difficulty ordering spec
```

Acceptance: (1) standard bundle is behavior-identical to today under seed;
(2) every archetype plays position-differentiated poker; (3) chips/100
ordering weak < standard < strong reproduces under a fixed seed; (4) invalid
YAML cannot ship (CI gate) and cannot brick a session (runtime fallback).

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
