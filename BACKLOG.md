# Backlog

> Refreshed 2026-07-15 by the `/backlog` skill. An index, not a spec — each item
> links to its source. Items tagged 🤖 were proposed by automated review, not
> authored by a human. Debt detail lives in [`docs/TECHNICAL_DEBT.md`](docs/TECHNICAL_DEBT.md).

Sources scanned: `docs/EPICS.md` + EPIC docs, `docs/known-issues.md`,
`CHANGELOG.md` (Unreleased), code markers (none found), GitHub issues (none open).

## EPICs / Features

| Item | Status | Notes |
|---|---|---|
| [EPIC-48 — Real Equity in the Browser](docs/EPIC-48_Real_Equity_WASM.md) | Planned; **Phase 0 unblocked** | Phase 0 spike is the next unblocked EPIC work. Phase 1 blocked on upstream pkcore EPIC-36. |
| [EPIC-49 — Data-Driven Bot Lineup & Difficulty](docs/EPIC-49_Bot_Lineup_Difficulty.md) | Planned | Phases 1–2 (YAML lineup, position awareness) need nothing upstream. Phase 3 tiers want 47/48. |
| [EPIC-47 — Adaptive Bots](docs/EPIC-47_Adaptive_Bots_Player_Stats.md) tail | Done, one deferral | Trained-`ExploitConfig` YAML deferred until an upstream EPIC-28 artifact exists. |

Suggested order per `docs/EPICS.md`: 46 → 47 → **48 Phase 0** → **49 Phase 1–2**,
converging on 47 Phase 3–4 / 48 / 49 Phase 3 as upstream EPIC-36 lands. EPIC-46
and EPIC-47 are complete, so the frontier is 48 Phase 0 and 49 Phase 1.

### EPIC-48 Phase 0 — Spike (next up)
- [ ] 0a. Enable `equity` in `Cargo.toml`; `make build`.
- [ ] 0b. Wasm-exposed probe: `compute()` on heads-up + 4-way flop spots at
      samples ∈ {500, 2000, 10000}; time via `performance.now()`. Confirm rayon
      serial fallback doesn't panic; record numbers in the EPIC doc.
- [ ] 0c. Choose the browser sample budget (target ≤10ms/decision at Turbo).

### EPIC-49 Phase 1 — YAML lineup
- [ ] 1a. `data/bots/standard.yaml` capturing today's nine profiles.
- [ ] 1b. `include_str!` + parse + fallback; replace the hardcoded pool.
- [ ] 1c. Extend the YAML validation bin + Makefile hook.
- [ ] 1d. Seeded regression: standard.yaml ≡ `default_profiles()`.

## Bugs

- [ ] **Game state resets when the page is backgrounded (mobile)** — high on
  mobile, low on desktop. **Deferred 2026-04-25**; owner prefers option A (full
  in-progress-hand preservation), which likely needs upstream pkcore serde
  support. First step is checking whether `PokerSession` / `BotProfile` already
  derive `Serialize`/`Deserialize`. ([`docs/known-issues.md`](docs/known-issues.md))
- [ ] 🤖 **Saved "adaptive bots" preference never reaches WASM on page load** —
  silent no-op from a load-order bug; UI and engine disagree.
  (`www/js/main.js:1077` — detail in [`docs/TECHNICAL_DEBT.md`](docs/TECHNICAL_DEBT.md))

## Tech debt

Seven 🤖 findings from the seeding review — see
[`docs/TECHNICAL_DEBT.md`](docs/TECHNICAL_DEBT.md). Headlines: no native tests
for the `#[wasm_bindgen]` exports; a stranded-session path in `next_hand()`;
duplicated bootstrap between `init_game`/`init_bot_game`; undocumented
Play/Arena module-isolation trick; `www/js/main.js` monolith.

## Docs / Notes

- [ ] 🤖 `docs/known-issues.md` forced-fold entry describes work EPIC-46 already
  fixed — move it under a "Resolved" heading.
- [ ] 🤖 `docs/known-issues.md` "State surface" table is stale post-design2.0
  (9 cells listed vs 13 actual; points at `www/index.html` instead of
  `www/js/main.js`).

## Housekeeping

- **Unmerged work.** Branch `EPIC-46` carries all of EPIC-46 + EPIC-47 (11
  commits) ahead of `main`. Both EPICs are marked Done — this wants a PR.
- **Unreleased changelog.** `CHANGELOG.md` has an `[Unreleased]` section (P&L
  tracker, score-bar rework) pending a version bump since 0.1.0.
