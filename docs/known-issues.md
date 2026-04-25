# Known Issues

A log of known defects in pkarena0-web. Each entry is self-contained:
symptom, repro, root cause, fix direction, severity. Add new entries at
the top; don't rewrite older ones once a fix lands — strike them through
or move them under a "Resolved" section so the history stays readable.

---

## Game state resets when the page is backgrounded (mobile)

**First reported:** 2026-04-25

### Symptom

If the user navigates away from the game tab — switching apps on mobile,
locking the screen, opening a new tab — and then returns later, the
in-progress hand and session are gone. The board is reset, blinds are
back to seat 1, the chip stacks are back to $10,000, and the hand log is
empty. Most reproducible on iOS Safari and Android Chrome under memory
pressure (e.g. after browsing other apps in between).

### Reproduction

1. Open <https://imperialbower.github.io/pkarena0-web/> on an iPhone or
   Android device.
2. Play through a few hands so the score bar shows non-zero PnL and the
   hand log has entries.
3. Switch to another app (or several) and use them for a minute or two
   to put the browser tab under memory pressure. Locking the screen and
   waiting also works.
4. Return to the browser and the pkarena0 tab.
5. Observe: the page has reloaded from scratch. PnL is `$0`, hand count
   is `0`, the table is freshly seated, and any in-progress hand is
   lost.

Desktop browsers reproduce this only rarely — they generally keep
focused tabs in memory. Forcing a full reload (Cmd-R) on desktop also
triggers the symptom.

### Root cause

The entire game state lives in Rust `thread_local!` singletons inside
the WASM module. JavaScript calls a small set of exported functions
(`init_game`, `step_bot`, `human_action`, `next_hand`, `get_state`,
`get_session_yaml`) and receives JSON back — no state is held on the JS
side, and nothing is persisted across page loads.

When a mobile OS evicts the backgrounded tab to reclaim memory, the
browser later restores the tab by reloading the page. That re-runs
`init_game(gameSeed)` (called at `www/index.html:1535`, `:1671`, and
`:2117`) which constructs a fresh game from a fresh seed. The Rust
`thread_local!` singletons start empty. There is no restore path.

Confirming evidence:

- The only `localStorage` use in `www/index.html` is the `audioEnabled`
  toggle (lines 1249, 2332). No game state is ever written.
- There are zero `visibilitychange`, `pagehide`, `pageshow`,
  `beforeunload`, or `freeze`/`resume` handlers in the file. Nothing
  attempts to snapshot state before eviction or detect a BFCache
  restore.
- The PWA manifest (`www/manifest.json`) does not change this — adding
  a manifest does not exempt a page from tab eviction.

### Fix direction

Two complementary pieces, both straightforward:

1. **Snapshot on backgrounding.** Add a `visibilitychange` handler that
   calls the existing `get_session_yaml()` export when the document
   becomes hidden, and writes the result to `localStorage` (or
   IndexedDB if it's too large for the 5 MB localStorage budget).
   `pagehide` is the safer mobile-Safari choice — `visibilitychange`
   doesn't always fire before iOS evicts a tab.

2. **Restore on init.** Add a Rust export — call it
   `restore_from_yaml(yaml: &str) -> Result<String, JsValue>` — that
   parses a previously-exported session YAML and rehydrates the
   `thread_local!` state. On page load, before calling `init_game`,
   check `localStorage` for a snapshot; if present and the schema
   version matches, restore from it instead of starting a new game.

Half the serialization plumbing already exists (`get_session_yaml` is
the YAML export feature). The new work is the inverse parser on the
Rust side and the lifecycle wiring on the JS side.

The BFCache path (`pageshow` event with `event.persisted === true`)
needs no work — when the browser keeps the page in memory and restores
it cheaply, the WASM module's state is preserved automatically. The
defect only matters for the full-reload-after-eviction path.

### Severity

- **High on mobile.** The product is positioned as "plays in any
  browser, mobile or desktop" (description in `www/index.html:8`). On
  mobile, the eviction-and-reset cycle can happen within minutes of
  inattention and silently destroys the session.
- **Low on desktop.** A focused desktop tab is rarely evicted; the bug
  is mostly only reachable via manual reload.
- **No data corruption.** The reset is clean — there's no inconsistent
  state, just lost progress.
