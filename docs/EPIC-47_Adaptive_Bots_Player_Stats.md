# EPIC-47: Adaptive Bots — Player Stats & Exploitative Play

> **One-line:** Enable pkcore's complete-but-dormant opponent-modeling stack
> (EPIC-26 `StatsRegistry` → EPIC-27 `ExploitativeDecider` → EPIC-28 trained
> `ExploitConfig`s) so the bots adapt to how the human actually plays — plus a
> HUD so the human can see the same stats.

## Status

| Component | Status |
|---|---|
| Enable `player-stats` feature in `Cargo.toml` | **Complete** (`Cargo.toml:18`) |
| Identity threading: `from_table_state_with_ids` + per-player snapshot | **Complete** (`src/lib.rs:421,553` — pkcore `PlayerSnapshot`, not the sketched 5-tuple; see corrigendum §1) |
| `StatsRegistry` thread_local; `ingest_hand` per completed hand | **Complete** (`src/lib.rs:82,569`) |
| `TableSnapshot::from_table_with_stats` in `step_bot` | **Complete** (`src/lib.rs:1323`) |
| Rust tests: identity attribution + unwrapped-decider no-op regression | **Complete** (`src/lib.rs:2654,2817`) |
| Wrap bot deciders in `ExploitativeDecider` (EPIC-27) | **Complete** (`make_bot_seat`, `src/lib.rs:1710`) |
| Adaptivity toggle (`set_adaptive` export + Settings checkbox) | **Complete** (`src/lib.rs:170,177`; `www/index.html:133`; `www/js/main.js`) |
| Rust test: adaptation diverges post-gate, neutral without stats | **Complete** (`adaptive_wrapping_tests`, `src/lib.rs:2907,3017`) |
| Ship an EPIC-28 trained `ExploitConfig` as embedded YAML | **Deferred** — using `ExploitConfig::default()`; no trained artifact exists yet (training is offline/native) |
| Per-seat HUD badges (VPIP/PFR/AF, dimmed while low-confidence) | **Complete** (`HudStats`, `src/lib.rs:1240`; `renderHud`, `www/js/table.js:88`) |

**Closed 2026-07-16** (branch `EPIC-46`, at `c6e855f`) — Phases 1–4 shipped;
Work Item 3b deferred until an EPIC-28 artifact exists. See the
[corrigendum](#implementation-corrigendum-2026-07-16-branch-epic-46).

**Depends on:** [EPIC-46](EPIC-46_Decider_Integration.md) (decider-per-seat
architecture — required to wrap deciders and inject stats).
**Supersedes / absorbs:** `docs/FEATURE_player_stats.md` (0.0.51-era plan;
its HUD design carries forward as Phase 4 here, and its "bots won't adapt —
out of scope" caveat is exactly what this EPIC removes).

---

## Context

pkcore 0.2.1 ships the full adaptive stack, all marked **Complete** upstream:

- **EPIC-26** — `PlayerStats` (`src/analysis/player_stats.rs:55`): VPIP, PFR,
  3-bet%, fold-to-c-bet, AF, WTSD, … each `Option<f64>` plus a
  `Confidence` tier (`:201`) gating on sample size. `StatsRegistry` (`:256`)
  keyed by `Uuid`, fed by `ingest_hand(&HandHistory)` (`:306`).
- **EPIC-27** — `ExploitativeDecider<D>`
  (`src/bot/exploitative_decider.rs:44`): wraps any `BotDecider`; when the
  snapshot carries `opponent_stats`, applies `adjust_profile`
  (`src/bot/exploit.rs:197`) driven by `ExploitConfig` (`exploit.rs:36`, 8
  deviation rules with min-hands gates). No stats attached ⇒ exact no-op.
- **EPIC-28** — `ExploitTrainer` produced tuned `ExploitConfig`s,
  YAML-serializable behind the `bot-training` feature (training happens
  offline, native-side; the web app only *consumes* a trained YAML).

None of it reaches pkarena0-web today, for three independent reasons:

1. **Feature off.** `Cargo.toml` enables only `bot-profiles, hand-histories`.
   (Verified 2026-07-12: `cargo check --target wasm32-unknown-unknown
   --no-default-features --features bot-profiles,hand-histories,equity,player-stats`
   passes cleanly on pkcore 0.2.1.)
2. **No identities.** The hand-history snapshot is a 4-tuple
   (`src/lib.rs:307`) and `HandHistory::from_table_state` (`src/lib.rs:413`)
   leaves every `player_id` as `None` — a `StatsRegistry` would have nothing
   to correlate on. `Player.id: Uuid` exists
   (pkcore `src/casino/table/player.rs:29`) and
   `from_table_state_with_ids` (pkcore `src/hand_history.rs:207`) is the
   intended entry point.
3. **No injection seam.** Pre-EPIC-46, the web app calls
   `BotProfile::decide`, which builds its snapshot without stats and cannot
   be wrapped.

The feed already exists: `COLLECTION: HandCollection` (`src/lib.rs:55`)
receives every completed hand (`src/lib.rs:426`). This EPIC taps that feed.

### Why this matters for gameplay

Today a maniac and a nit get identical treatment from the bots. With this
EPIC, a human who folds to every c-bet gets barreled more; a calling station
gets value-bet thinner and bluffed less. This is the single biggest
"the bots feel alive" upgrade available at zero algorithmic cost — the
algorithms shipped upstream months ago.

---

## Goals

- Bots adapt to observed play (human *and* other bots — the registry is
  seat-agnostic; arena mode gets bot-vs-bot adaptation for free).
- Human sees a per-seat HUD once `Confidence` clears `Low`.
- Zero behavior change when the adaptivity toggle is off or sample sizes are
  below `ExploitConfig` min-hands gates.
- No persistence writes (wasm): registry is session-scoped. (localStorage
  persistence is an open question, not a goal.)

## Scope

**In scope:** feature flag, identity threading, registry wiring, decider
wrapping, one embedded trained `ExploitConfig`, HUD badges, toggle.

**Out of scope:** running `ExploitTrainer` in the browser;
`player-stats-persistence` (uses `std::fs` — will not link on wasm);
`SimTable` (batch simulator, not the interactive loop); new stats beyond
what `PlayerStats` derives.

---

## Design

### Identity threading (the load-bearing step)

Extend `player_snapshot` (`src/lib.rs:307`) from
`Vec<(u8, String, usize, Option<String>)>` to
`Vec<(u8, String, usize, Option<String>, Option<Uuid>)>` appending
`Some(seat.player.id)`, and switch `src/lib.rs:413` to
`from_table_state_with_ids`. Without this, every downstream piece is inert.

Bot `Uuid`s must be stable across hands within a session — verify the pool
construction (`src/lib.rs:93-96,146-149`) reuses `Player` instances rather
than reminting ids per hand.

### Registry

```rust
thread_local! {
    static REGISTRY: RefCell<StatsRegistry> = RefCell::new(StatsRegistry::new());
}
// next_hand(), right after COLLECTION.push (src/lib.rs:426):
REGISTRY.with(|r| r.borrow_mut().ingest_hand(&hh));
```

Reset alongside `COLLECTION` at session start (`src/lib.rs:117,163`).

### Injection + wrapping (builds on EPIC-46)

```rust
// step_bot, replacing the plain snapshot:
let snapshot = TableSnapshot::from_table_with_stats(&table, seat, Some(&registry));
let act = bot.decider.decide_seeded(&bot.profile, &snapshot, rng);
```

Bot construction wraps when adaptivity is on:

```rust
Box::new(ExploitativeDecider::wrap_with_config(RuleBasedDecider, trained_config))
// joker: ExploitativeDecider::wrap_with_config(JokerDecider::default(), ...)
```

`ExploitConfig` YAML ships as an embedded `include_str!` asset (same pattern
as other static data), parsed once at session start. Start with pkcore's
default config; swap in an EPIC-28 trained one after arena validation.

### HUD (Phase 4 — carried over from FEATURE_player_stats.md)

Per-seat badges (VPIP / PFR / AF) serialized through `get_state()` JSON
(`stats: { vpip, pfr, af, confidence, hands }` on each `PlayerView`) and
rendered into the existing `.seat-hud` / `.list-hud` elements
(`data-seat="{n}"` convention).

**Gating (as built, correcting the original sketch):** `Confidence::Low` is the
*lowest/default* tier (0–49 hands), so a `>= Confidence::Low` test is always
true. The badge is instead gated on **stats existing at all** — `stats` is
`None` until the seat's identity has a completed hand in the registry, which is
what keeps it absent at hand 1. `confidence` then drives *styling*, not
presence: `hud-low` badges are dimmed to flag a small-sample read.

---

## Work Items

### Phase 1 — Plumbing (no behavior change) ✅
- [x] 1a. Add `player-stats` to the pkcore features in `Cargo.toml:18`.
- [x] 1b. Identity threading: per-player snapshot carrying `Uuid` +
  `from_table_state_with_ids` (`src/lib.rs:421,553`). Landed on pkcore's own
  `PlayerSnapshot` struct (`src/lib.rs:28`) rather than the sketched 5-tuple —
  corrigendum §1.
- [x] 1c. `REGISTRY` thread_local (`src/lib.rs:82`); ingest per hand
  (`src/lib.rs:569`); reset per session (`src/lib.rs:254,307`).
- [x] 1d. Test: after N seeded hands, `registry.get(bot_id)` returns stats
  with plausible VPIP for a loose vs tight archetype
  (`stats_registry_correlates_players_by_identity`, `src/lib.rs:2654`).

### Phase 2 — Stats reach the deciders ✅
- [x] 2a. `from_table_with_stats` in `step_bot` (EPIC-46 seam,
  `src/lib.rs:1323`).
- [x] 2b. Regression: unwrapped `RuleBasedDecider` ignores stats — seeded
  action sequences unchanged by Phase 1+2a alone
  (`stats_bearing_snapshot_does_not_change_rule_based_action`, `src/lib.rs:2817`).

### Phase 3 — Adaptation on ✅
- [x] 3a. Wrap deciders in `ExploitativeDecider::wrap_with_config` behind a
  Settings toggle (`set_adaptive` / `adaptive_enabled` WASM exports +
  `#adaptive-toggle` checkbox). Default **on for both modes** (resolved from
  the Open Question); the value is read when the lineup is built, so a change
  applies on the next New Game / Start Arena. `make_bot_seat(profile, adaptive)`
  (`src/lib.rs:1710`) wraps `RuleBasedDecider` / `JokerDecider` alike.
  Two follow-on fixes landed after the phase: the saved preference is pushed to
  WASM on load (`d325987`) and the live flag is surfaced in `get_state()` for
  engine/UI lockstep (`44302d8`, `adaptive_enabled`, `src/lib.rs:177`).
- [~] 3b. **Deferred.** Landed with `ExploitConfig::default()` — the doc's
  "start with the default config, swap in a trained one after arena validation"
  path. Embedding a trained YAML (with `default()` parse-failure fallback) waits
  on an actual EPIC-28 artifact; the `wrap_with_config` seam already accepts one.
- [x] 3c. Seeded, deal-independent test
  (`adaptive_wrapping_diverges_after_gate_and_is_neutral_without_stats`): a
  fixed flop spot with an authored calling-station registry. Asserts (1) the
  wrapper routes decisions through the stat-adjusted profile, (2) ≥1 decision
  differs from the unwrapped baseline once the min-hands gate clears, and (3)
  the wrapper is a byte-for-byte no-op when no stats are attached.

### Phase 4 — HUD ✅
- [x] 4a. Serialize per-seat `{vpip, pfr, af, confidence, hands}` in
  `get_state()` (`HudStats`, `src/lib.rs`). Emitted only when the seat's
  identity has ≥1 completed hand (`hands_dealt > 0`), so it is absent at hand 1.
  The hero seat carries its own stats too (the "human stats in the HUD" nicety).
- [x] 4b. UI badges (`renderHud` in `www/js/table.js`), formatted `VPIP/PFR/AF`
  (percent/percent/ratio; `·` for an as-yet-uncomputable stat). Gating note:
  `Confidence::Low` is the *default/lowest* tier (0–49 hands), so `>= Low`
  is always true — the real "no badge yet" gate is *stats existence*
  (empty registry until a hand completes). Confidence instead drives styling:
  `hud-low` badges are dimmed to mark a small-sample read as provisional.
- [x] 4c. Playwright (`tests/hud.spec.ts`): zero visible badges on a fresh
  play-mode boot (hand 1); after an instant-speed arena run, active bots show a
  three-field `VPIP/PFR/AF` badge.

---

## Key Files

Line references are as of close (`c6e855f`, 2026-07-16); `## Context` and
`## Design` keep their audit-time (2026-07-12) coordinates — see EPIC-46's
corrigendum §4 for the convention.

| File | Role |
|---|---|
| `Cargo.toml:18` | `player-stats` feature |
| `src/lib.rs:28,421,553` | `PlayerSnapshot` import/build + `from_table_state_with_ids` |
| `src/lib.rs:76,82,254,307,569` | collection/registry lifecycle + `ingest_hand` |
| `src/lib.rs:1257,1323` | `step_bot` — `from_table_with_stats` injection |
| `src/lib.rs:97,170,177,1710` | `ADAPTIVE` flag, toggle exports, decider wrapping |
| `src/lib.rs:1240` | `HudStats` — per-seat stats in `get_state()` |
| `www/js/table.js:88` | `renderHud` — VPIP/PFR/AF badges |
| `www/index.html:133` | `#adaptive-toggle` Settings checkbox |
| pkcore `src/analysis/player_stats.rs:55,256` | `PlayerStats`, `StatsRegistry` |
| pkcore `src/bot/exploitative_decider.rs:44` | wrapper |
| pkcore `src/bot/exploit.rs:36,197` | `ExploitConfig`, `adjust_profile` |
| pkcore `src/bot/table_snapshot.rs:325` | `from_table_with_stats` |
| `www/` (state renderer) | HUD badges |

---

## Verification

```bash
cargo check --target wasm32-unknown-unknown   # feature builds on wasm (pre-verified)
cargo test                                     # registry + adaptation gates
make build && npx playwright test              # HUD + arena adaptation specs
```

Acceptance: (1) stats accumulate with correct identity attribution;
(2) adaptation provably changes ≥1 decision post-gate and zero decisions
pre-gate / when off; (3) HUD respects `Confidence`; (4) wasm bundle still
builds and the game loop's per-step latency is visually unchanged.

---

## Implementation corrigendum (2026-07-16, branch `EPIC-46`)

Phases 1–4 shipped; Work Item 3b deferred. Verified at close: `cargo test` →
**20 passed, 0 failed, 5 ignored** (the 5 are EPIC-49's `#[ignore]`d fixture
generators and entropy-dealt benches, not skipped EPIC-47 work).

1. **The 5-tuple became pkcore's `PlayerSnapshot`.** The design extended an
   ad-hoc `Vec<(u8, String, usize, Option<String>)>` to a 5-tuple with
   `Option<Uuid>` appended. `from_table_state_with_ids` in fact takes
   `&[PlayerSnapshot]` — a named pkcore struct (`pkcore::hand_history`,
   imported `src/lib.rs:28`) — so the web app builds those directly
   (`src/lib.rs:421`) instead of growing a tuple. Better outcome than sketched:
   the identity field is typed and named upstream rather than positional here.
2. **Ingest happens *before* `COLLECTION.push`, not after.** The design sketch
   put `ingest_hand` "right after `COLLECTION.push`". That cannot compile:
   `HandCollection::push` takes `hh` **by value** and moves it, so the registry
   must borrow first. Shipped order is `ingest_hand(&hh)` then `push(hh)`
   (`src/lib.rs:569,570`), with a comment at the site recording why.
3. **Stats inherit the chip-audit guard.** Hand histories — and therefore stats
   ingestion — are skipped entirely when a hand fails pkcore's chip audit
   (`had_audit_failure`, `src/lib.rs:515,552`), a defense-in-depth holdover from
   the pkcore ≤0.0.53 `ChipAuditFailed` era. Unstated in the design; the
   consequence is that an audit-failed hand contributes nothing to a player's
   sample, which is the correct bias (better a missing hand than a corrupt one).
4. **Work Item 3b deferred — `ExploitConfig::default()` shipped instead.** The
   EPIC scoped "one embedded trained `ExploitConfig`" (EPIC-28 artifact, YAML via
   `include_str!`). No such artifact exists — training is offline/native-only —
   so the wrapper runs pkcore's default config. The `wrap_with_config` seam
   already accepts a trained one, so adoption is a data drop, not a refactor.
   This is the EPIC's one unmet scope item, and it does not gate the rest: the
   default config's deviation rules are what Phase 3c's divergence test proves.
5. **The HUD gate is stats-existence, not `Confidence`.** The design gated badges
   on `Confidence` clearing `Low`. `Confidence::Low` is the *default/lowest*
   tier (0–49 hands), so `>= Low` is always true and would have shown a badge at
   hand 1. The shipped gate is whether the seat's identity has a completed hand
   in the registry at all (`stats` is `None` until then); `confidence` instead
   drives *styling* — `hud-low` badges are dimmed to mark a small-sample read as
   provisional (`renderHud`, `www/js/table.js:88`). Acceptance #3 ("HUD respects
   `Confidence`") is met in this corrected sense, not as literally written.
6. **Adaptation's value is human-modeling, and the bench says so.** EPIC-49's
   matchup bench measured adaptive-wrapped standard profiles as a consistent
   mild *drag* bot-vs-bot (−2.7k and −3.8k chips/100 over two 96k-hand runs; see
   EPIC-49 corrigendum §1). That is not a defect in this EPIC — a bot-vs-bot
   bench cannot see the human tendencies `ExploitConfig` exists to punish — but
   it is why adaptation stayed a **user toggle** rather than becoming EPIC-49's
   strong-tier lever, and why EPIC-49 forces it off on the weak tier.
7. **Default-on resolved to both modes** (Open Question 1, struck below), with
   two follow-on fixes after Phase 4: the saved preference now reaches WASM on
   load (`d325987`) and the live flag is surfaced in `get_state()` (`44302d8`)
   so engine and UI cannot silently disagree.
8. **Inherited debt / handoffs:** the trained-`ExploitConfig` drop (§4) is the
   live thread — it needs an upstream EPIC-28 run, not work here. Registry
   persistence across sessions stays deferred (Open Question 2). The registry is
   session-scoped by design: reset alongside `COLLECTION` at every session start
   (`src/lib.rs:255,308`).

| Phase | Status at close |
|---|---|
| Phase 1 — plumbing | **Complete** |
| Phase 2 — stats reach the deciders | **Complete** |
| Phase 3 — adaptation on | **Complete** (3b deferred — no EPIC-28 trained artifact exists) |
| Phase 4 — HUD | **Complete** (gating corrected — §5) |

---

## Open Questions

- **Default-on where?** ~~Resolved (Phase 3): **on for both modes** by
  default, overridable via the Settings toggle.~~ Adaptation is gated by the
  registry's min-hands thresholds, so early hands play identically to today
  regardless. If EPIC-49 ships a difficulty selector, it can drive
  `set_adaptive` (and later a per-lineup `ExploitConfig`) as one of its levers.
- **Registry persistence across sessions** via localStorage
  (`StatsRegistry` is serde-able)? Deferred — session-scoped is enough to
  demonstrate adaptation, and cross-session stats raise "it remembers me"
  UX questions.
- **Human stats in the HUD too?** The registry tracks the human seat as well;
  showing the player their own VPIP/PFR is a nice mirror. Cheap once Phase 4
  lands.
