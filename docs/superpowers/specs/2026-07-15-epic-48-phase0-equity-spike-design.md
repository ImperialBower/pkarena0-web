# EPIC-48 Phase 0 — Equity Spike (Design)

> Spike to answer whether pkcore's `EquityRequest` engine is viable inside
> threadless WASM, characterize its per-decision latency, and pick a Monte
> Carlo sample budget. Companion to
> [`docs/EPIC-48_Real_Equity_WASM.md`](../../EPIC-48_Real_Equity_WASM.md)
> Work Items 0a–0c. **Throwaway measurement code, feature-gated so it never
> ships in a release build.**

## Goal

Produce data, not features. Three questions:

1. **No-panic:** does `EquityRequest::compute()` run under `wasm32` with no
   OS threads, given the engine calls `rayon`'s `into_par_iter()`
   (`pkcore .../equity/engine.rs:189`) unconditionally?
2. **Latency:** per-decision wall time across realistic spots and sample
   budgets, on a desktop floor **and** a 4× CPU-throttled mobile proxy.
3. **Budget:** what `max_samples` keeps a decision within ~10ms at 4× throttle?

The output is a results table pasted into EPIC-48 plus a recorded budget
decision — enough to flip Phase 0's Status rows and unblock the go/no-go.

## Findings that shape the spike (established during brainstorming)

- **`equity` is already enabled** in `Cargo.toml` (line 17) and the default
  wasm bundle already compiles with it. Item **0a is effectively done**; the
  spike is really 0b (probe + measure) and 0c (pick a budget).
- **Random villains force Monte Carlo on every street.** The engine only picks
  exact enumeration when *every* seat has known cards
  (`engine.rs:38`, `exact_enumerate` at `:161`). In a real bot decision the
  opponents' cards are unknown → `PlayerSpec::Random`, so the engine enumerates
  villain holdings too and exceeds `exact_threshold` even on the flop.
  Therefore `max_samples` (the 0c budget) governs latency on **all** streets,
  and the realistic spot to model is **`Exact(hero) + Random villains`** — which
  also removes all card-collision bookkeeping (only the hero's two cards and
  the board must be collision-free).
- The exact-enumeration path still exists (all seats known, near-showdown), so
  the probe measures it once for completeness, but it is not the hot path.

## API surface (pkcore 0.2.1, verified against the crates.io source)

```rust
use pkcore::analysis::equity::{EquityRequest, EquityOptions, PlayerSpec};
use pkcore::arrays::two::Two;
use pkcore::play::board::Board;
use std::str::FromStr;

let req = EquityRequest {
    players: vec![PlayerSpec::Exact(Two::HAND_AS_KS), PlayerSpec::Random, /* … */],
    board: Board::from_str("Qh Jc 2d").unwrap(), // or Board::default() preflop
    opts: EquityOptions { max_samples: 2_000, seed: Some(7), ..Default::default() },
};
let report = req.compute()?; // EquityReport { players: Vec<PlayerEquity>, method, .. }
```

- `EquityOptions` default: `exact_threshold: 100_000`, `max_samples: 100_000`,
  `seed: None`. The probe always sets `seed: Some(_)` for determinism.
- `EquityReport` carries the `Method` the engine actually chose (exact vs MC) —
  the probe records it per spot so we can *see* which path ran, not assume.
- `PlayerEquity::equity_pct()` gives a scalar for the sanity/determinism check.

## Deliverable

### 1. Feature-gated probe

- Add a `equity-probe` feature to `Cargo.toml` (`[features]`, off by default).
- New module `src/equity_probe.rs`, included from `src/lib.rs` behind
  `#[cfg(feature = "equity-probe")]`, exporting one `#[wasm_bindgen]` function:

  ```rust
  #[wasm_bindgen]
  pub fn equity_probe() -> String  // returns JSON (see schema below)
  ```

  Keeping it in its own module + behind the feature means the default
  `make build` bundle is byte-for-byte unchanged. Nothing throwaway leaks into
  the shipping surface.

### 2. Build target

- `Makefile` gains `equity-probe:` which runs
  `wasm-pack build --target web --out-dir www/pkg --features equity-probe`
  (builds the probe bundle in place). A plain `make build` afterward restores
  the clean bundle. Re-runnable on demand, including later on a real device.

### 3. Measurement matrix

Hero `= Exact(Two::HAND_AS_KS)` throughout. Villains `= Random`. Flop board
`= "Qh Jc 2d"` (collision-free with the hero).

| # | Spot | Seats | Board | Sweep | Expected `Method` |
|---|------|-------|-------|-------|-------------------|
| 1 | HU preflop        | hero + 1 Random | empty | `max_samples ∈ {500, 2000, 10000}` | MC |
| 2 | 4-way preflop     | hero + 3 Random | empty | same sweep | MC |
| 3 | 4-way flop        | hero + 3 Random | flop  | same sweep | MC |
| 4 | 6-way flop        | hero + 5 Random | flop  | same sweep | MC |
| 5 | HU exact (control)| hero + 1 Exact (`Two::HAND_AH_AD`) | flop | n/a (single run) | Exact |

Spot 5 uses a **flop** board deliberately: an all-known spot *preflop* has a
~1.7M board-runout space (`C(48,5)`) that exceeds `exact_threshold`, so the
engine would pick MC. On a flop the runout is `C(45,2)=990 ≤ 100_000`, which is
what actually exercises the exact-enumeration path (`exact_enumerate`,
`engine.rs:161`). Its villain cards (`Ah Ad`) are collision-free with the hero
(`As Ks`) and the board (`Qh Jc 2d`).

For each `(spot, samples)` cell the probe: runs a warm-up call, then **20**
timed iterations via `web_sys::Performance::now()`, and records **median** and
**p95** milliseconds, the chosen `Method`, and one equity value (for the
sanity/determinism check). Iteration count is a constant, easy to bump.

### 4. Driver spec

`tests/equity-probe.spec.ts`, gated so the normal suite skips it:

```ts
test.skip(!process.env.EQUITY_PROBE, 'requires a --features equity-probe build');
```

It assumes a probe bundle is already built (`make equity-probe`), then:

1. Loads the page, waits for boot.
2. Calls `equity_probe()` **unthrottled**, parses the JSON.
3. Opens a CDP session, `Emulation.setCPUThrottlingRate {rate: 4}`, calls
   `equity_probe()` again.
4. **Asserts every spot returned a result (no panic / no `Err`)** — this is the
   0b no-panic gate.
5. Determinism: asserts two seeded runs of spot 1 return identical equity.
6. Prints both result tables (unthrottled + 4×) to stdout for transcription.

Run with: `make equity-probe && EQUITY_PROBE=1 npx playwright test equity-probe`.

### JSON schema returned by `equity_probe()`

```json
{
  "rayon_ok": true,
  "results": [
    { "spot": "hu_preflop", "seats": 2, "board": "", "samples": 500,
      "method": "MonteCarlo", "median_ms": 0.0, "p95_ms": 0.0,
      "hero_equity_pct": 0.0 }
  ]
}
```

`rayon_ok` is `true` if all `compute()` calls returned `Ok`; the probe catches
nothing (a panic aborts the module), so a returned JSON with `rayon_ok: true`
is itself the proof that the serial-fallback path did not panic.

## Outputs → EPIC-48 (0b / 0c)

- Paste the unthrottled + 4× tables into EPIC-48's "Open (Phase 0 spike)"
  section; flip the three Status rows (feature enable, runtime spike, latency
  budget) from Planned to Done/measured.
- Record the chosen `max_samples` budget with reasoning, per street if they
  differ (preflop vs flop+ cost differently).
- **Branch condition:** if even 500-sample MC exceeds the ~10ms 4× budget at
  4- or 6-way, that is the trigger for the doc's documented fallback — preflop
  via the embedded `hup_cache`, MC only on flop+ — and/or a lower per-street
  sample count. The spike *surfaces* this; it does not decide the decider
  wiring (that is Phase 1, blocked on upstream pkcore EPIC-36).

## Scope / non-goals

**In:** the feature-gated probe, the build target, the gated driver spec, the
measurement, and the recorded numbers + budget in EPIC-48.

**Out:** any change to `RuleBasedDecider` or the live bot decision path (that is
Phase 1, upstream-blocked); real-device mobile measurement (deferred — the
gated probe makes it a cheap follow-up); web workers / threaded wasm; caching.

## Verification

1. `make build` — clean bundle still builds, no `equity-probe` symbols in it.
2. `make equity-probe` — probe bundle builds green.
3. `EQUITY_PROBE=1 npx playwright test equity-probe` — every spot returns a
   result under both throttle settings (no panic), determinism holds, tables
   printed.
4. `npx playwright test` (no env) — the probe spec is skipped; the existing
   suite is unaffected.
5. EPIC-48 updated with the numbers and the budget decision.

Acceptance mirrors EPIC-48 Phase 0: (1) spike numbers recorded, no panic;
(2) a sample budget chosen with reasoning; (3) the default shipping bundle is
unchanged (probe fully feature-gated).
