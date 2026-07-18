# EPIC-51: `strengthen()` Isolation & Re-scope

> **One-line:** Now that EPIC-50's `equity` knob is the **dominant** strong-tier
> lever (it roughly tripled the tier's edge), measure how much `strengthen()`'s
> *manual* tuning still contributes — then keep it, thin it to its earning parts,
> or retire it in favor of the knobs.

## Status

All **Planned** — a follow-up to EPIC-50's open tuning question (EPIC-50 §Design
revision, §corrigendum-5). No work has started.

| Component | Status |
|---|---|
| Bench isolating `strengthen`'s marginal contribution (strengthen+equity vs equity-only) | Planned |
| Decision from the data: keep / thin / retire `strengthen()` (`src/lib.rs:1965`) | Planned |
| Re-scope implementation (whichever the data dictates) | Planned |
| Re-validate `make bench-tiers` ordering + `bot_bundle_fixture` parity | Planned |
| Docstring + EPIC-50 corrigendum reconciled | Planned |

---

## Context

EPIC-50 wired pkcore 0.3.0's `decision:` knobs into the difficulty bundles. The
strong tier is `strengthen()` (`src/lib.rs:1965-2032` — tight ~10% opening range,
bluff clamp ≤8, `value_threshold: 0.5`, position grades) **plus**
`strong_decision()` (`equity: fast{500}` + `ranges: position_aware`).

The EPIC-50 bench measured the *combined* strong tier at **+67,450 chips/100** over
standard — nearly 3× the pre-knob `strengthen`-only figure (≈+24k). That proves
equity is the dominant lever, but it does **not** isolate `strengthen`'s marginal
contribution *now that equity is on*: the two could be additive, redundant, or —
because `strengthen` tightens the opening range while `ranges: position_aware`
supplies a different positional range — even mildly at odds.

EPIC-50 explicitly left this open (§Design revision "Open tuning question", §4b):
*"if the knobs preserve `strong > standard` with margin, `strengthen()` is reduced
to the bluff-clamp + `value_threshold` (or retired). We do not guess — we
measure."* This EPIC is that measurement and the cleanup it implies.

### This EPIC does **not**

- Change the `equity` / `ranges` knobs or the tier layout — only `strengthen()`.
- Touch the weak or standard tiers.
- Re-open the deferred `outs` / `preflop_charts` knobs (that is upstream pkcore
  EPIC-39).

---

## Goals

- **Measure** `strengthen()`'s marginal chips/100 with the equity knob already on.
- **Decide** its fate on the evidence: keep as-is, thin to the parts that earn, or
  retire (strong tier = standard-pool profiles + `strong_decision()`).
- **Re-validate** the tier ordering and parity gate after the change.
- Leave the strong tier at least as strong as EPIC-50 shipped it.

## Scope

**In scope:** a bench variant isolating `strengthen`; the resulting re-scope of
`strengthen()` (`src/lib.rs`); re-running `make bench-tiers`; updating the parity
fixture (`strong_bundle_matches_strengthened_pool`) and docstrings.

**Out of scope:** the knobs themselves; the other tiers; upstream pkcore work.

**Rule:** whatever the decision, the strong tier's measured edge over standard
must not regress below EPIC-50's baseline beyond bench σ.

---

## Design

### The isolation bench

Add a matchup variant to `difficulty_ordering_tests` (`src/lib.rs:3383`):
`strengthen + strong_decision` (the shipped strong tier) vs `strong_decision`
only (equity + position ranges on the *un-strengthened* standard-pool profile).
The delta is `strengthen`'s marginal contribution once equity is present.

```rust
// strong (shipped)      = with_decision(strengthen(std), strong_decision())
// equity-only baseline  = with_decision(std,             strong_decision())
```

Reuse `run_matchup` / `pool_profile` / `CORE_ARCHETYPES` (`src/lib.rs`), the
cash-mode reset harness EPIC-50 already benches with. Compute-bound (both sides
run equity), so keep the hand count at the EPIC-50 strong level (~12k) and treat
it as an `#[ignore]`d statistical bench like its siblings.

### The three outcomes

| Measured `strengthen` margin | Action |
|---|---|
| clearly positive (≫ σ) | **Keep** `strengthen()` as-is; document it earns its keep alongside equity. |
| near zero | **Thin** to just the parts that move the number (likely the bluff-clamp + `value_threshold`); drop the manual range-tightening now that `ranges: position_aware` + equity carry hand selection. |
| negative (fights the knobs) | **Retire**: strong tier = standard-pool profile + `strong_decision()`; `builtin_strong_pool` drops the `strengthen` map. |

We pick from the number, not a priori.

---

## Work Items

### Phase 1 — Measure
- [ ] 1a. Add the `strengthen+equity` vs `equity-only` matchup to
  `difficulty_ordering_tests`; run via `make bench-tiers`; record the delta and σ.

### Phase 2 — Decide & re-scope
- [ ] 2a. Apply the outcome (keep / thin / retire) to `strengthen()` and, if
  retiring, to `builtin_strong_pool` (`src/lib.rs:1943`).
- [ ] 2b. Update the parity fixture assertions
  (`strong_bundle_matches_strengthened_pool`, `src/lib.rs`) to match the new
  strong pool; regenerate `data/bots/strong.yaml`.

### Phase 3 — Re-validate & document
- [ ] 3a. `make bench-tiers`: strong > standard still holds with margin; record.
- [ ] 3b. Update `strengthen()`'s docstring and the EPIC-50 corrigendum §5 with the
  isolated number and the decision taken.

---

## Test Plan

| Test | Asserts |
|---|---|
| new `strengthen_marginal_contribution` (bench, `#[ignore]`) | isolates `strengthen`'s chips/100 delta with equity on |
| `strong_tier_beats_standard_tier` (kept) | strong > standard still holds post-re-scope |
| `bot_bundle_fixture::strong_bundle_matches_strengthened_pool` | parity holds for whatever the strong pool becomes |

Determinism note: entropy-dealt, so statistical with σ-margin and `#[ignore]`d,
per EPIC-49 §3 / EPIC-50.

## Key Files

| File | Role |
|---|---|
| `src/lib.rs` (`strengthen`, `1965`) | the transform under evaluation |
| `src/lib.rs` (`builtin_strong_pool`, `1943`) | strong pool construction, if retiring |
| `src/lib.rs` (`difficulty_ordering_tests`, `3383`) | the isolation bench |
| `src/lib.rs` (`strong_bundle_matches_strengthened_pool`) | parity fixture to update |
| `data/bots/strong.yaml` | regenerated if the strong pool changes |

## Reuse (do NOT recreate)

- `run_matchup`, `pool_profile`, `CORE_ARCHETYPES`, `bare` — the EPIC-50 bench
  harness.
- `strong_decision()` — the knob config; unchanged.

## Compatibility

- The weak and standard tiers are untouched.
- If `strengthen()` is thinned/retired, `data/bots/strong.yaml` regenerates and
  the parity gate updates in lockstep — no drift.

## Dependencies

- **Built on:** **EPIC-50** (the knobs + the real-pool bench) and **EPIC-49** (the
  tiers + `strengthen`/`weaken`).
- **Related:** upstream pkcore **EPIC-39** (range model) — independent; this EPIC
  is purely about the local `strengthen()` layer.

## Verification

```bash
make bench-tiers        # isolation bench + ordering, entropy-dealt statistical
cargo test --lib bot_bundle   # parity gate for the (possibly changed) strong pool
cargo test --lib        # fast suite stays green
```

Acceptance: (1) `strengthen`'s marginal contribution is measured and recorded;
(2) the re-scope decision follows the number; (3) strong > standard holds with
margin after the change; (4) the parity gate and fast suite stay green;
(5) `strengthen()`'s docstring and the EPIC-50 corrigendum reflect the outcome.
