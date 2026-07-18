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

> **Historical — this is the 2026-07-12 audit snapshot that motivated the set,
> not current state.** Every gap below except the EPIC-36-blocked row has since
> closed; all four EPICs are closed as of 2026-07-16. The "Used by web bots?"
> column and the `src/lib.rs:1087` coordinate describe the code *before*
> EPIC-46 — that call site no longer exists. See the per-EPIC Status tables for
> what is true now.

At audit time, every bot action in this app flowed through **one call** —
`bot.decide(&table, seat, rng)` at `src/lib.rs:1087` — which hardcoded
pkcore's `RuleBasedDecider` on its weakest settings. What that left on the
table, given pkcore 0.2.1's shipped capabilities:

| pkcore capability | Upstream status | Used by web bots? *(audit, 2026-07-12)* | Gap → EPIC |
|---|---|---|---|
| `BotDecider` trait, `on_new_hand` lifecycle, `JokerDecider` | Complete | ✗ (trait bypassed; joker plays as static GTO) | [EPIC-46](EPIC-46_Decider_Integration.md) |
| `PlayerStats` / `StatsRegistry` (EPIC-26) | Complete | ✗ (`player-stats` feature off; hand histories carry no UUIDs) | [EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) |
| `ExploitativeDecider` + `ExploitConfig` (EPIC-27/28) | Complete | ✗ (nothing to wrap; no stats attached) | [EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) |
| Multi-way `EquityRequest` engine (exact + seeded MC) | Complete (`equity` feature) | ✗ (bots use hand-rank proxy / preflop coin-flip) | [EPIC-48](EPIC-48_Real_Equity_WASM.md) |
| `Outs`/`CaseEvals` draw equity, graded `PotOdds` discipline | Upstream EPIC-36 (2026-07-17): graded `PotOdds` discipline **wired**; `Outs`/`CaseEvals` draw equity **deferred upstream** (needs villain cards the decider never sees) | ✗ | [EPIC-48](EPIC-48_Real_Equity_WASM.md) |
| Embedded HUP preflop odds (`hup_cache`, wasm-safe) | Complete | ✗ | [EPIC-48](EPIC-48_Real_Equity_WASM.md) |
| `Playbook` / `PositionRanges` position awareness | Complete | Partial (3 of 8 archetypes) | [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) |
| `BotProfile` YAML round-trip (`bot-profiles`) | Complete | ✗ (feature on, never used — lineup hardcoded) | [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) |
| Graded `decision:` capability knobs | **EPIC-36: shipped upstream in pkcore 0.3.0** (2026-07-17) — `equity` / `ranges` / `pot_odds` / `exploit` wired into `RuleBasedDecider`; `outs` + `preflop_charts` deferred upstream | — | [EPIC-48](EPIC-48_Real_Equity_WASM.md) / [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) adopt in the (now-unblocked) follow-up |
| CFR `Solver` at runtime | Complete upstream | ✗ — **deliberately**; too slow for live play (upstream ruling) | none (non-goal) |
| `SimTable`, `store` (SQLite/BCM), stats persistence | Complete | ✗ — **deliberately**; batch/native-only or `std::fs` (won't link on wasm) | none (non-goal) |

Key wasm fact (verified 2026-07-12): pkcore 0.2.1 **compiles cleanly for
`wasm32-unknown-unknown`** with `bot-profiles, hand-histories, equity,
player-stats` — the gaps were wiring, not platform blockers. All four features
ship enabled today (`Cargo.toml:14-19`). The open runtime question (rayon
serial fallback + equity latency in-browser) was **answered** by EPIC-48
Phase 0: no panic, serial fallback confirmed, budget set at 500 MC samples.

## The EPICs

| EPIC | Title | Depends on | Status |
|---|---|---|---|
| [EPIC-46](EPIC-46_Decider_Integration.md) | Full BotDecider Integration | — | **Closed 2026-07-16** — both phases complete; repair ladder pulled forward into scope |
| [EPIC-47](EPIC-47_Adaptive_Bots_Player_Stats.md) | Adaptive Bots — Player Stats & Exploitative Play | 46 | **Closed 2026-07-16** — Phases 1–4 complete; trained-`ExploitConfig` YAML deferred until an EPIC-28 artifact exists |
| [EPIC-48](EPIC-48_Real_Equity_WASM.md) | Real Equity in the Browser | 46; upstream pkcore EPIC-36 | **Closed 2026-07-16** — Phase 0 delivered (500-sample MC budget, probe removed); Phases 1–2 deferred to a follow-up EPIC, now **unblocked**: upstream EPIC-36 shipped the `equity` knob 2026-07-17 |
| [EPIC-49](EPIC-49_Bot_Lineup_Difficulty.md) | Data-Driven Bot Lineup & Difficulty | 46 (consumes 47) | **Closed 2026-07-16** — Phases 1–3 complete (three parity-gated bundles, position awareness for all, difficulty selector, chips/100 bench `make bench-tiers`); `decision:`-knob adoption deferred to EPIC-48's follow-up, now **unblocked** (upstream EPIC-36 shipped 2026-07-17) |
| [EPIC-50](EPIC-50_Decision_Knob_Adoption.md) | Decision-Knob Difficulty Adoption | 48, 49 (pkcore 0.3.0) | **Complete 2026-07-17** — `ranges` on standard, `equity + ranges` on strong; bench (real pools) standard +23.8k / strong +67.5k chips/100; browser-latency spec passes. `outs`/`preflop_charts`/tier-lever `exploit` out of scope |
| [EPIC-51](EPIC-51_Strengthen_Rescope.md) | `strengthen()` Isolation & Re-scope | 50 | **Planned** — the EPIC-50 follow-up: measure `strengthen()`'s marginal contribution now equity is the dominant strong lever, then keep / thin / retire it on the evidence |

Suggested order — **completed as planned**: 46 → 47 → 48 Phase 0 →
49 Phases 1–3. The one remaining thread — upstream pkcore EPIC-36 — **shipped
in pkcore 0.3.0** (2026-07-17: `equity` / `ranges` / `pot_odds` / `exploit`
wired; `outs` + `preflop_charts` deferred upstream), and this repo's dependency
is now pinned to `0.3.0` (`Cargo.toml:14`; native + `wasm32` checks pass). The
follow-up EPIC is now unblocked: it picks up EPIC-48 Phases 1–2 (equity
adoption) and lands the `decision:` knobs in EPIC-49's bundles. Note
`outs`/`preflop_charts` will remain unavailable until an upstream range-model
lets the decider price draws without villain cards — tracked as pkcore
[EPIC-39](../../pkcore/docs/EPIC-39_Decider_Range_Model.md); scope no bundle that
depends on them until it ships.

## Relationship to existing docs

- [`FEATURE_player_stats.md`](FEATURE_player_stats.md) — 0.0.51-era HUD plan;
  absorbed into EPIC-47 (its HUD design = Phase 4; its "bots won't adapt —
  out of scope" caveat is what EPIC-47 removes).
- [`FEATURE_pnl.md`](FEATURE_pnl.md) — shipped; the chips/100 arena report in
  EPIC-49 builds on the same session-accounting surface.
- pkcore [`ROADMAP.md`](../../pkcore/ROADMAP.md) — upstream EPIC index;
  EPIC-34 (variant selection in this app) is upstream-tracked and untouched
  by this set.
