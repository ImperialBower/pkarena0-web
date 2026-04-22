# Audio Layer — Integration Guide

**Status:** Phase 1 scaffolding (voice stitcher + live event adapter)
**Scope:** Turn pkarena0-web into an audio-first, blindfold-playable Hold'em game
**Location in repo:** `www/audio/`

---

## What this is

A drop-in audio layer for pkarena0-web that narrates every game action and
exposes on-demand state queries via keyboard. It has **no dependencies** —
just Web Audio, SpeechSynthesis, and `fetch` — and makes no changes to the
Rust core for Phase 1.

The layer is built from three independent pieces that share one event shape:

```
  Rust/WASM get_state() ──▶  LiveAdapter  ──▶  GameEvent stream  ──▶  Voice (speaks)
                                                     ▲                 ▲
                   YAML replay file  ──▶  ReplayAdapter   (Phase 2)    │
                   All-bot tab      ──▶  BotStreamAdapter (Phase 3)    │
                                                                ARIA live region
                                                                Hand log UI
```

Because every source produces the same `GameEvent` shape, the voice layer,
hotkeys, ARIA mirror, and hand log are written once and work for live play,
replays, and bot-only sessions.

---

## What's included in Phase 1

| File                          | Purpose                                               |
| ----------------------------- | ----------------------------------------------------- |
| `www/audio/voice.js`          | Voice clip stitcher + SpeechSynthesis fallback        |
| `www/audio/adapters/live.js`  | Polls `get_state`, diffs, emits `GameEvent`s          |
| `docs/voice-script.md`        | Script for voice actors (154 atomic clips)            |
| `www/audio/voice/*.wav`       | Recorded clips *(to be produced by voice actors)*     |

Not yet included (see **Next steps**):

- `www/audio/adapters/replay.js` — YAML hand-history playback
- `www/audio/adapters/bot_stream.js` — all-bot fast-narration mode
- `www/audio/hotkeys.js` — on-demand queries (H, B, P, S, W, L, ?, Esc)
- `www/audio/aria.js` — visually hidden live region for screen readers
- `www/audio/profile.js` — verbosity/speed/pan user settings

---

## Repository layout after integration

```
pkarena0-web/
├── Cargo.toml
├── src/                          (unchanged — Rust core)
├── www/
│   ├── index.html                ← add <script type="module"> (see below)
│   ├── pkg/                      (wasm-pack output, unchanged)
│   └── audio/                    ← NEW
│       ├── voice.js
│       ├── adapters/
│       │   └── live.js
│       └── voice/                ← NEW: .wav files drop in here
│           ├── num_zero.wav
│           ├── seat_one.wav
│           └── …154 total
└── docs/
    ├── voice-script.md           ← NEW
    └── audio-integration.md      ← this file
```

Put both JS modules in `www/audio/` rather than at the web root so they stay
isolated from the existing UI code and can be lifted into another project
unchanged.

---

## Integration steps

### 1. Drop the files in

```
mkdir -p www/audio/adapters www/audio/voice docs

cp voice.js                www/audio/voice.js
cp live.js                 www/audio/adapters/live.js
cp pkarena0-voice-script.md docs/voice-script.md
cp this-file.md            docs/audio-integration.md
```

Commit with no wiring yet. These files are inert until imported.

### 2. Expose the WASM state accessor to JS

pkarena0-web already does this — you already call `get_state()` from the
frontend. The adapter just needs a zero-arg function that returns the parsed
state. If the existing binding returns a JSON string, wrap it:

```js
const getState = () => JSON.parse(wasm.get_state());
```

If it already returns an object (via `serde-wasm-bindgen`), pass it directly.

### 3. Wire it into `index.html`

Add a single module script. Existing UI code is untouched.

```html
<script type="module">
  import init, * as wasm      from './pkg/pkarena0_web.js';   // existing
  import { Voice }             from './audio/voice.js';
  import { LiveAdapter }       from './audio/adapters/live.js';

  let voice, adapter;

  // Browsers require a user gesture before AudioContext can play. Hook
  // initialisation to the existing "New Game" button — don't create the
  // AudioContext on page load.
  document.querySelector('#new-game').addEventListener('click', async () => {
    await init();  // existing WASM init

    if (!voice) {
      voice = new Voice({ basePath: './audio/voice/' });
      const { loaded, missing } = await voice.preload();
      console.log(`[audio] ${loaded} clips loaded, ${missing} missing`);
      window.voice = voice;  // handy for console testing
    }

    if (!adapter) {
      adapter = new LiveAdapter({
        getState:   () => JSON.parse(wasm.get_state()),
        onEvent:    handleEvent,
        intervalMs: 100,
      });
      adapter.start();
      window.adapter = adapter;
    }

    voice.say.line('sess_new_game');
  });

  // Optional but recommended: right after a human or bot action, poke the
  // adapter so narration fires immediately instead of on the next poll tick.
  // Wherever you currently call human_action() or step_bot(), add:
  //   adapter?.poke();

  function handleEvent(ev) {
    switch (ev.kind) {
      case 'hand_start':
        voice.say.line('deal_new_hand');
        break;
      case 'deal':
        if (ev.data.to === 'hero') voice.say.yourHand(ev.data.cards);
        break;
      case 'street':
        if (ev.data.street === 'flop')  voice.say.flop(ev.data.board);
        if (ev.data.street === 'turn')  voice.say.turn(ev.data.board[3]);
        if (ev.data.street === 'river') voice.say.river(ev.data.board[4]);
        break;
      case 'action':
        voice.say.seatAction(ev.seat, ev.data.verb, ev.data.amount);
        break;
      case 'your_turn':
        voice.say.yourTurn({
          toCall: ev.data.to_call,
          pot:    ev.data.pot,
          stack:  ev.data.stack,
        });
        break;
      case 'showdown':
        if (ev.data.winners[0]) {
          voice.say.showdownWin(ev.data.winners[0].seat, []);
        }
        break;
    }
  }
</script>
```

### 4. First smoke test (no WAV files needed)

Phase 1 runs even before any clips are recorded — the stitcher falls back to
`SpeechSynthesis` for any missing atoms, so you'll hear synthesized narration
right away.

1. Build WASM: `wasm-pack build --release --target web --out-dir www/pkg`
2. Serve: `python3 -m http.server 8080 --directory www`
3. Open `http://localhost:8080`
4. Click **New Game**
5. Open the browser console. You should see:
   - `[audio] 0 clips loaded, 154 missing` (expected — no recordings yet)
   - After each bot action, a line from the diff engine
6. Listen: every bot action should be narrated via your OS's default voice.

### 5. Fix field names against the real JSON

The live adapter's extractor guesses at field names like `hand_id`, `pot`,
`to_act`, `action_log`. Some of those guesses will be wrong. To pin them down:

```js
// In the console, after clicking New Game:
JSON.stringify(JSON.parse(wasm.get_state()), null, 2);
```

Paste the output into a GitHub issue or a file, then open
`www/audio/adapters/live.js` and edit the `Extract.*` methods to match. Each
method has a `pick(raw, 'candidate.path', 'alt.path')` call — update the
paths. Also check the warning output:

```
[live-adapter] no hand_id in get_state payload; using 0
```

Any such warning points at a field the extractor couldn't find.

### 6. Record the voice clips (or ship with TTS)

Give `docs/voice-script.md` to a voice actor. 154 clips, ~2–3 hours of studio
time. Deliverables go into `www/audio/voice/` with the exact filenames from
the script.

Until recordings arrive, the system is fully usable on TTS — ship now,
upgrade audio later. A background `preload()` call after recordings land will
swap in the higher-quality clips with zero code change.

### 7. Add a `poke()` call where bots step (recommended)

In whatever function animates bot actions at 1-second intervals, call
`window.adapter?.poke()` immediately after each `step_bot()` call. This
makes narration fire at the instant the state updates instead of up to 100 ms
later. Not required, but noticeably crisper.

---

## Design constraints & things to know

### Browser gesture requirement

`AudioContext` can't play audio until the user interacts with the page. The
integration above piggybacks on the existing **New Game** button click, which
is already a gesture. If you add other entry points (replay launcher, bot
tab), each needs a user-triggered handler to call `voice.preload()` or
`new AudioContext()`.

### Stitched speech vs full phrases

Every narration is assembled at runtime from atoms
(`seat_three` + `act_raises_to` + `num_four` + `num_hundred`). This keeps the
clip inventory to 154 files and supports arbitrary bet amounts. The tradeoff
is slight choppiness between atoms. Section 11 of `voice-script.md` is an
optional polish pass (48 extra clips) that records full seat-action phrases.

### Amount range

Number decomposition in `voice.js` handles 0 through 999,999 with "and"
placement. Blinds in pkarena0-web are fixed at $50/$100, so this covers any
session comfortably. If later modes introduce six-figure blinds, extend
`numberToAtoms()` — the script already includes "thousand."

### Polling vs callback

The adapter polls `get_state` every 100 ms by default. That's fine for live
play. The all-bot tab will generate events much faster than polling can
resolve; the Phase 3 plan is to add a WASM → JS callback (`set_on_state_change`)
so pkcore pushes instead of JS pulling. Small Rust-side change.

### Replay timing

Every `GameEvent` carries `t_ms` (ms since adapter start). The replay adapter
will use these timestamps to schedule playback at 1×, 2×, 4× speed, and will
support scrubbing by ID. Nothing downstream needs to change to support this.

---

## Event shape (the one contract everything shares)

```js
{
  id:      42,              // monotonic across the session
  t_ms:    12840,           // ms since session start
  hand_id: 7,               // which hand
  kind:    'action',        // see below
  seat:    3,               // 0 = hero, 1..8 = bots; null for global events
  data:    { …kind-specific… }
}
```

Kinds and their data payloads:

| kind         | data                                                                             |
| ------------ | -------------------------------------------------------------------------------- |
| `hand_start` | `{ button_seat, sb_seat, bb_seat, stacks: {seat: amount} }`                      |
| `deal`       | `{ to: 'hero'\|'board', cards: ['As','Kd'], street }`                            |
| `action`     | `{ verb, amount, pot_after, to_call_after }`                                     |
| `street`     | `{ street: 'flop'\|'turn'\|'river', board, pot }`                                |
| `your_turn`  | `{ to_call, pot, min_raise, stack }`                                             |
| `showdown`   | `{ shown: [{seat, cards, hand_rank_text}], winners: [{seat, amount, rank_text}]}`|
| `hand_end`   | `{ chip_delta: {seat: delta} }`                                                  |

---

## Next steps

### Phase 2 — Hotkey queries
**Blocker:** none. Can start today.
Add `www/audio/hotkeys.js`:

- `H` — speak hero's hole cards
- `B` — speak the board
- `P` — speak pot and amount-to-call
- `S` — speak hero's stack (and all stacks in seat order)
- `W` — speak whose turn it is, panned to that seat
- `L` — replay the last spoken line
- `?` — full state summary
- `Esc` — cancel current speech

Needs one small addition to `LiveAdapter`: a `getSnapshot()` method that
returns the current normalized snapshot. Trivial to add.

### Phase 3 — Replay adapter
**Blocker:** decide on replay source format.
Two options:

1. Parse the existing YAML hand-history export. Pro: no Rust change. Con:
   YAML has to be walked to reconstruct events; timing info is implicit.
2. Add `session_log_json()` to pkcore alongside `session_yaml()`, emitting
   the exact `GameEvent` list. Pro: replay is trivial and reuses the same
   shape. Con: requires a small Rust-side addition.

Recommendation: option 2. It also doubles as the all-bot tab's data source.

### Phase 4 — All-bot fast-narration mode
**Blocker:** Phase 3, plus a Rust-side change for speed.

Two sub-modes:

- **Full narration, sped up** (4× playback). Uses the replay adapter's event
  stream from a running bot session.
- **Batch mode** — one summary event per hand:
  `"Hand 47: seat 3 wins 1,240 with two pair, aces and kings."`

Profile flag: `batch_hands: true` in `profile.js`.

### Phase 5 — Accessibility polish

- ARIA live region (`www/audio/aria.js`) mirroring every announcement as
  visually hidden text. Lets screen-reader users play with their own TTS
  voice and rate.
- User-facing settings panel exposed by `,` key:
  - Master volume
  - Pan intensity (0–1, for users who find hard-left/right disorienting)
  - TTS rate
  - Verbosity (verbose / earcons-only / silent)
  - Enable/disable ARIA mirror
- Persist settings to `localStorage` via `profile.js`.

### Phase 6 — Polish pass (optional)

- Record Section 11 of the voice script (per-seat action phrases) for
  smoother stitching.
- Add chord stingers for win/lose/all-in events (synthesized via Web Audio
  oscillators — no new recordings).
- Add an earcon vocabulary for cards (suit timbre + rank pitch) as a faster
  alternative to spoken card names. Toggled by a profile flag.

---

## Testing

Both modules have standalone unit tests that run under Node without a
browser:

```
cd tests/audio/
node voice.test.mjs   # number & card decomposition
node live.test.mjs    # diff engine over a full synthetic hand
```

(These test files are not yet committed — port them from the scratch work
when integrating. They're small: ~50 lines each.)

Playwright tests for browser-side audio can assert on the ARIA live region's
text content once Phase 5 ships. That's the most reliable way to test audio
output in CI without dealing with actual sound playback.

---

## Questions / decisions to resolve

1. **Replay format** — YAML parsing or add `session_log_json()`?
2. **Bot personalities** — if bots get names/styles, swap "Seat three" for
   names in the voice script. Decide before recording.
3. **Rust-side callback hook** — add `set_on_state_change(js_fn)` to avoid
   polling? Nice-to-have for live, required for the all-bot tab.
4. **Event persistence** — should `GameEvent`s be saved alongside the YAML
   hand history for exact-replay? If yes, the simplest path is
   `session_log_json()` returning the event array.

---

## License

Same as the parent project: GPL-3.0-or-later.
