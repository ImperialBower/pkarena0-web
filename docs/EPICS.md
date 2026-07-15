# pkarena0-web EPICs — Bot Capability Roadmap

Index of pkarena0-web's EPIC documents. Numbers are allocated from the shared
ImperialBower EPIC sequence (pkcore hosts 00–43 incl. cross-repo pointers;
pkdealer holds 44–45; **pkarena0-web starts at 46**).

Source: a bot-capability audit of pkarena0-web against pkcore 0.2.1
(2026-07-12). Companion to pkcore's own
[EPIC-36 audit](../../pkcore/docs/EPIC-36_Configurable_Bot_Capabilities.md),
which found the upstream deciders use ~20% of pkcore's decision toolbox —
this repo consumes even less of it.

## The gap map

Every bot action in this app flows through **one call** —
`bot.decide(&table, seat, rng)` at `src/lib.rs:1087` — which hardcodes
pkcore's `RuleBasedDecider` on its weakest settings. What that leaves on the
table, given pkcore 0.2.1's shipped capabilities:

| pkcore capability | Upstream status | Used by web bots? | Gap → EPIC |
|---|---|---|---|
| `BotDecider` trait, `on_new_hand` lifecycle, `JokerDecider` | Complete | ✗ (trait bypassed; joker plays as static GTO) | [EPIC-46](EPIC-46_Decider_Integration.md) |
| `PlayerStats` / `StatsRegistry` (EPIC-26) | Complete | ✗ (`player-stats` feature off; hand histories carry no UUIDs) | [EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) |
| `ExploitativeDecider` + `ExploitConfig` (EPIC-27/28) | Complete | ✗ (nothing to wrap; no stats attached) | [EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) |
| Multi-way `EquityRequest` engine (exact + seeded MC) | Complete (`equity` feature) | ✗ (bots use hand-rank proxy / preflop coin-flip) | [EPIC-48](EPIC-48_Real_Equity_WASM.md) |
| `Outs`/`CaseEvals` draw equity, graded `PotOdds` discipline | Complete (wiring = upstream EPIC-36) | ✗ | [EPIC-48](EPIC-48_Real_Equity_WASM.md) |
| Embedded HUP preflop odds (`hup_cache`, wasm-safe) | Complete | ✗ | [EPIC-48](EPIC-48_Real_Equity_WASM.md) |
| `Playbook` / `PositionRanges` position awareness | Complete | Partial (3 of 8 archetypes) | [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) |
| `BotProfile` YAML round-trip (`bot-profiles`) | Complete | ✗ (feature on, never used — lineup hardcoded) | [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) |
| Graded `decision:` capability knobs | **EPIC-36: Planned upstream** | — | [EPIC-48](EPIC-48_Real_Equity_WASM.md) / [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) adopt on release |
| CFR `Solver` at runtime | Complete upstream | ✗ — **deliberately**; too slow for live play (upstream ruling) | none (non-goal) |
| `SimTable`, `store` (SQLite/BCM), stats persistence | Complete | ✗ — **deliberately**; batch/native-only or `std::fs` (won't link on wasm) | none (non-goal) |

Key wasm fact (verified 2026-07-12): pkcore 0.2.1 **compiles cleanly for
`wasm32-unknown-unknown`** with `bot-profiles, hand-histories, equity,
player-stats` — the gaps are wiring, not platform blockers. The one open
runtime question (rayon serial fallback + equity latency in-browser) is
EPIC-48 Phase 0.

## The EPICs

| EPIC | Title | Depends on | Status |
|---|---|---|---|
| [EPIC-46](EPIC-46_Decider_Integration.md) | Full BotDecider Integration | — | Done |
| [EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) | Adaptive Bots — Player Stats & Exploitative Play | 46 | Done (Phases 1–4; trained-`ExploitConfig` YAML deferred until an EPIC-28 artifact exists) |
| [EPIC-48](EPIC-48_Real_Equity_WASM.md) | Real Equity in the Browser | 46; upstream pkcore EPIC-36 | Phase 0 done (500-sample MC budget); Phases 1–2 blocked on upstream EPIC-36 |
| [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) | Data-Driven Bot Lineup & Difficulty | 46 (consumes 47/48) | Phase 1 done (YAML lineup); Phases 2–3 remaining |

Suggested order: **46 → 47 (Phases 1–2) → 48 Phase 0 → 49 Phase 1–2**, then
converge on 47 Phase 3–4 / 48 / 49 Phase 3 as upstream EPIC-36 lands.

## Relationship to existing docs

- [`FEATURE_player_stats.md`](FEATURE_player_stats.md) — 0.0.51-era HUD plan;
  absorbed into EPIC-47 (its HUD design = Phase 4; its "bots won't adapt —
  out of scope" caveat is what EPIC-47 removes).
- [`FEATURE_pnl.md`](FEATURE_pnl.md) — shipped; the chips/100 arena report in
  EPIC-49 builds on the same session-accounting surface.
- pkcore [`ROADMAP.md`](../../pkcore/ROADMAP.md) — upstream EPIC index;
  EPIC-34 (variant selection in this app) is upstream-tracked and untouched
  by this set.
