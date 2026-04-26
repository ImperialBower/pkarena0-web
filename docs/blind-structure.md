# Blind Structure

The tournament blind schedule used by pkarena0-web. Defined client-side in
[`www/index.html`](../www/index.html) as the `BLIND_LEVELS` array (around
line 2268). Levels advance every **10 hands played** (not by wall-clock
time); the final level is terminal and applies for the rest of the
tournament.

| Level | Hands | SB    | BB    | BB Δ vs prior |
|-------|-------|-------|-------|---------------|
| 1     | 10    | 50    | 100   | —             |
| 2     | 10    | 100   | 200   | +100 (+100%)  |
| 3     | 10    | 150   | 300   | +100 (+50%)   |
| 4     | 10    | 200   | 400   | +100 (+33%)   |
| 5     | 10    | 300   | 600   | +200 (+50%)   |
| 6     | 10    | 400   | 800   | +200 (+33%)   |
| 7     | 10    | 500   | 1,000 | +200 (+25%)   |
| 8     | 10    | 750   | 1,500 | +500 (+50%)   |
| 9     | 10    | 1,000 | 2,000 | +500 (+33%)   |
| 10    | 10    | 1,500 | 3,000 | +1,000 (+50%) |
| 11    | 10    | 2,000 | 4,000 | +1,000 (+33%) |
| 12    | ∞     | 3,000 | 6,000 | +2,000 (+50%) |

Notes:

- No antes are configured.
- Levels 1–11 cover hands 1–110; from hand 111 onward the structure stays
  at level 12 (3,000 / 6,000).
- The frontend calls `mod.set_blinds(sb, bb)` on the WASM module whenever
  the level changes, so adjusting the schedule is a frontend-only edit.
