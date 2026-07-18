# EPIC-52: Bot Decision Transparency ("Bot X-Ray") — PRELIMINARY

> **One-line:** A user-facing debug toggle that reveals, per bot decision, what
> the bot *saw* (opponent stats, equity, pot odds), how it *adapted* (exploit
> deltas), and why it *acted* (which rule/knob fired, what the repair ladder
> clamped).

## Status

**Draft — scoping only.** This is a preliminary outline; no design decision
below is final and no work has started. The Phase 0 spike exists to turn the
open questions into a real plan.

| Component | Status |
|---|---|
| Phase 0 spike: inventory what is observable without upstream changes | Draft |
| Debug toggle + per-bot "inputs & adjustments" panel (Layer 1) | Draft |
| Upstream `DecisionTrace` proposal → pkcore EPIC (Layer 2) | Draft |
| Adaptation-over-time view ("how they are adjusting") | Draft |

---

## Context

Every bot capability shipped in EPIC-46..50 is invisible at the table. A user
watching a strong-tier bot cannot tell that it just ran a 500-sample equity
estimate, that `ExploitativeDecider` loosened its calling range because the
human is folding 80% of hands, or that the repair ladder clamped an illegal
raise. The bots got smart; the UI still shows only the resulting chip motion.

This EPIC adds an observability mode: a toggle (persisted like the adaptive
toggle, EPIC-47 Phase 3) that opens a per-bot explanation surface.

### The hard constraint

`BotDecider::decide()` returns a **bare `PlayerAction`**
(pkcore `src/bot/decider.rs:89`) — no rationale comes back. The interesting
internals (`hand_equity`, `proxy_equity`, `exploit_profile`,
`preflop_open_frequency`) are private free functions inside pkcore.

Worse, the decision path is RNG-coupled: `decide_seeded` consumes the shared
entropy RNG (`src/lib.rs:1332`). The UI **cannot** shadow-recompute equity to
explain a decision — it would either perturb the RNG (changing the game) or use
different samples (explaining a decision the bot never made). *Any true "why"
must be emitted from inside the actual decision path.* That means an upstream
pkcore change, same pattern as EPIC-36: this repo writes the need, pkcore
ships the capability.

### What IS observable locally today (no upstream work)

- The exact `TableSnapshot` + attached opponent-stats the decider saw
  (`src/lib.rs:1312` builds it; EPIC-47 HUD already reads the registry).
- Whether the seat is adaptive, and its `ExploitConfig` (canonical config at
  wrap time, `src/lib.rs:1716-1723`).
- The tier's `decision:` knobs (equity samples, ranges mode — EPIC-50).
- Repair-ladder events: the raw action the decider proposed vs. what the
  ladder legalized (EPIC-46 — this is pkarena0-web code, fully traceable).
- The chosen action, pot, price to call → pot odds are arithmetic on known
  state.

---

## Goals (draft)

- A **toggle** ("Bot X-Ray" / debug mode) the user can flip mid-session;
  off = zero overhead, identical gameplay.
- Per bot decision, show **inputs** (stats snapshot, knobs, position, pot
  odds), **adjustments** (exploit deltas vs. base profile), and — once
  upstream trace lands — **reasoning** (equity computed, range verdict, rule
  fired).
- An **adaptation view**: how a bot's read on each opponent has shifted over
  the session (stats deltas, confidence tier changes).
- **Determinism guarantee:** the mode observes; it never re-runs any decider
  logic. Toggling it must not change a single dealt card or action.

## Non-goals (draft)

- Not a strategy tutor / coaching feature (could be a later EPIC on top).
- Not hand-history persistence of traces (maybe later; start live-only).
- No CFR/solver introspection (solver is a non-goal per EPICS.md).

---

## Design sketch — three layers

| Layer | What the user sees | Needs upstream? |
|---|---|---|
| **1. Inputs & adjustments** | Per-bot card: archetype, active knobs, the opponent-stats snapshot it saw, adaptive on/off + exploit config, pot odds faced, repair-ladder clamp log | **No** — all observable in pkarena0-web now |
| **2. Decision trace** | "Computed equity 0.43 (500 MC) · range: call region · pot odds 0.28 graded OK · exploit: raise-size +12% vs. station" — emitted from inside `RuleBasedDecider` | **Yes** — new pkcore `DecisionTrace` API (sibling EPIC upstream) |
| **3. Adaptation timeline** | Sparkline/log per opponent: VPIP/PFR/confidence evolving, exploit deltas over hands | No (registry snapshots per hand), but richer with Layer 2 |

Candidate upstream shape (to be negotiated in the pkcore EPIC): a
non-breaking `decide_traced(...) -> (PlayerAction, DecisionTrace)` with a
default impl that returns an empty trace, so existing deciders are untouched;
trace entries as a small enum (EquityComputed, RangeVerdict, PotOddsGrade,
ExploitAdjustment, RuleFired). Cost concern: tracing must not slow the
untraced path — likely a no-op unless the app opts in.

Delivery surface: extend `get_state()` with an optional `debug` block when
the toggle is on (the JS layer already consumes `get_state()` JSON), rendered
as a side panel or per-seat popover. UI placement is an open question.

---

## Work Items (draft phasing)

### Phase 0 — Spike & upstream proposal
- [ ] 0a. Inventory exactly which Layer-1 fields are reconstructible with zero
  divergence risk; prototype the `debug` block in `get_state()`.
- [ ] 0b. Draft the pkcore `DecisionTrace` EPIC (number from the shared
  sequence) with the wasm/perf constraints; align with upstream.

### Phase 1 — Toggle + Layer 1 panel
- [ ] 1a. Persisted toggle (mirror the adaptive-toggle pattern, EPIC-47).
- [ ] 1b. Per-bot inputs/adjustments panel + repair-ladder event log.
- [ ] 1c. Regression spec: toggling X-Ray mid-hand changes no game state.

### Phase 2 — Trace adoption *(blocked on upstream EPIC)*
- [ ] 2a. Adopt `decide_traced` at the call site (`src/lib.rs:1332`); render
  the reasoning line per decision.

### Phase 3 — Adaptation timeline
- [ ] 3a. Per-opponent stats/confidence history across the session; surface
  "how the bot's read on you has changed."

---

## Open Questions

1. **Shadow vs. trace:** is any Layer-2 information safely derivable locally
   (e.g. cloning RNG state pre-decision), or do we hold the line that all
   reasoning comes from upstream trace? *(Current lean: hold the line — RNG
   cloning is fragile and duplicates equity cost.)*
2. **UI real estate:** side panel, per-seat popover, or a scrolling
   commentary log? Multi-bot tables produce a lot of decisions.
3. **Perf:** does building the debug block every action (stats snapshot
   serialization) need gating to only-when-toggled? (Almost certainly yes.)
4. **History:** live-only, or should traces attach to hand histories for
   post-hoc review? (Interacts with pkcore `hand-histories`.)
5. **Human players:** does the X-Ray also show what bots believe about the
   *human* (their stats profile)? Likely yes — that's the fun part — but
   confirm it doesn't leak hole-card-dependent info the bot shouldn't have.

## Dependencies

- **Built on:** EPIC-46 (decider path + repair ladder), EPIC-47 (stats
  registry, adaptive wrap, HUD), EPIC-50 (decision knobs worth explaining).
- **Blocks/blocked:** Phase 2 blocked on a new upstream pkcore
  `DecisionTrace` EPIC (to be drafted in Phase 0b).

## Verification (sketch)

- Playwright spec: enable X-Ray, run a hand, assert the debug block appears
  and matches the acting seat; assert identical game trajectory with the
  toggle on vs. off is *not* asserted directly (entropy deal) — instead
  assert the toggle triggers no WASM state mutation calls.
- `cargo test --lib`: debug block serialization; repair-ladder events
  captured; no stats-registry mutation from read paths.
