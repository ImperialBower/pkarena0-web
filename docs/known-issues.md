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
2. Play through a few hands so the score bar shows a non-zero hand
   count and the hand log has entries.
3. Switch to another app (or several) and use them for a minute or two
   to put the browser tab under memory pressure. Locking the screen and
   waiting also works.
4. Return to the browser and the pkarena0 tab.
5. Observe: the page has reloaded from scratch. Hand count is `0`, the
   table is freshly seated, and any in-progress hand is lost.

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

### State surface

For anyone working on the fix, the live state is held in nine
`thread_local!` cells declared at `src/lib.rs:40–55`:

| Cell | Type | What it holds |
|---|---|---|
| `SESSION` | `Option<PokerSession>` | The live game (table, players, current hand, street, betting) |
| `BOTS` | `Vec<BotProfile>` | The 8 bot profiles drawn at session start |
| `RNG` | `SmallRng` | Seeded RNG used for shuffling and bot decisions |
| `PHASE` | `SessionPhase` | One of `Uninitialized` / `BotsActing` / `WaitingForHuman` / `HandComplete` / `SessionOver` |
| `HAND_START_CHIPS` | `Vec<(u8, usize)>` | Chip counts before blinds for the current hand |
| `COLLECTION` | `HandCollection` | Completed hand histories — exported by `get_session_yaml` |
| `LAST_ERROR` | `Option<String>` | One-shot error message for the UI |
| `LAST_HAND_RESULT` | `Option<Vec<PotResult>>` | One-shot showdown summary |
| `IS_ALL_BOT` | `bool` | Arena-mode flag |

**Important:** `get_session_yaml()` (`src/lib.rs:461-467`) serializes
*only* `COLLECTION` — i.e. the completed hand histories. It does **not**
capture `SESSION`, `BOTS`, `RNG`, `PHASE`, or any in-progress hand. So
the existing YAML export is not a snapshot of the live game; it's the
post-hand history file. Any fix that wants to preserve the live game
needs new serialization machinery.

`PokerSession`, `BotProfile`, and `HandCollection` come from the
external `pkcore` crate. Whether they implement
`serde::Serialize`/`Deserialize` is an open question — answering it is
the first step of any implementation effort here.

### Open question: scope of the fix

The fix shape depends on what the player should see when they return.
Three candidate scopes, in increasing fidelity:

**A. Continue the exact in-progress hand.** Same board, same bot
actions so far, same hole cards, same turn order. Highest fidelity.
Requires full `PokerSession` snapshot+restore — almost certainly
requires upstream changes to `pkcore` so that `PokerSession`,
`BotProfile`, and the bot RNG state implement `Serialize` /
`Deserialize`. Schema versioning becomes a real concern because the
snapshot becomes tightly coupled to internal pkcore types.

**B. Restore the session, deal a fresh hand.** Persist chip stacks,
bot lineup (names + profile selection), hand count, and the completed
hand history (`COLLECTION`). On restore, rebuild the
table with the saved stacks and start a new hand. The player loses the
mid-flight hand but keeps everything else. Implementable inside this
repo without upstream pkcore changes — define a new
`SessionSnapshot` type in `src/lib.rs` that holds only the
serializable fields, expose `get_snapshot_json()` and
`restore_from_snapshot(json: &str)` exports.

**C. Same as B, with a "Welcome back" toast** acknowledging that the
in-progress hand was lost. Smallest UX delta from B.

Independent of which scope is chosen, the lifecycle wiring is the
same:

- **Snapshot trigger.** `pagehide` is the safer mobile-Safari choice
  than `visibilitychange` — `visibilitychange` doesn't always fire
  before iOS evicts a tab. Belt-and-suspenders: also snapshot after
  every completed hand, so eviction without a clean backgrounding
  signal still recovers the last finished state.
- **Storage.** `localStorage` is fine for B (a `SessionSnapshot` JSON
  blob fits comfortably in the 5 MB budget). For A, the snapshot may
  grow with hand history and want IndexedDB.
- **Restore on init.** On page load, before calling `init_game`, check
  storage for a snapshot. If present and the schema version matches,
  restore. Otherwise fall through to the existing fresh-start path.
- **BFCache.** The BFCache path (`pageshow` event with
  `event.persisted === true`) needs no work — when the browser keeps
  the page in memory and restores it cheaply, the WASM module's state
  is preserved automatically. The defect only matters for the
  full-reload-after-eviction path.

### Severity

- **High on mobile.** The product is positioned as "plays in any
  browser, mobile or desktop" (description in `www/index.html:8`). On
  mobile, the eviction-and-reset cycle can happen within minutes of
  inattention and silently destroys the session.
- **Low on desktop.** A focused desktop tab is rarely evicted; the bug
  is mostly only reachable via manual reload.
- **No data corruption.** The reset is clean — there's no inconsistent
  state, just lost progress.

### Status

**Deferred 2026-04-25.** Owner's preference is option A (full
in-progress hand preservation), but this is parked for a later
session — likely depends on upstream `pkcore` changes that aren't
in scope right now.

Before picking this back up, the first concrete step is to check
whether `pkcore::casino::session::PokerSession` and
`pkcore::bot::profile::BotProfile` already implement
`serde::Serialize`/`Deserialize` (or could be made to without
significant work). The answer determines whether option A is a
reasonable single-repo project or a multi-repo coordination effort,
and that in turn decides whether to commit to A or fall back to B/C.
