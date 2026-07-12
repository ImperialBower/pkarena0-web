    import { Voice }       from '../audio/voice.js';
    import { LiveAdapter } from '../audio/adapters/live.js';
    import { createTable } from './table.js';
    import { makeCard }    from './cards.js';
    import { initReplay }  from './replay.js';
    import { initThemes, initDeckToggle, initViewToggle } from './themes.js';

    // The live design2.0 table instance (felt + 9 seats + center). Built once;
    // driven by table.update(buildTableView(state)) on every render.
    const table = createTable(document.getElementById('table-zone'));

    let _playMod = null, _arenaMod = null;
    let arenaGeneration = 0;

    // ── Constants ─────────────────────────────────────────────────────────────
    let BOT_ACTION_MS = 1000;   // pause between each bot action (configurable via settings)
    let HAND_COMPLETE_MS = 5000; // pause to show result before starting next hand (configurable)
    let gameSeed = 0;            // seed used for the current session (saved to URL)
    let arenaRunning = false;
    let playLoopRunning = false;
    let arenaBlindLevel = 0;
    let voice        = null;
    let playAdapter  = null;
    let arenaAdapter = null;
    let audioEnabled = localStorage.getItem('audioEnabled') === 'true';
    let playBlindLevel  = 0;
    const BOT_EMOJIS = {
      'gto':               '🤖',
      'tight_passive':     '🐢',
      'loose_aggressive':  '🔥',
      'tight_aggressive':  '🎯',
      'loose_passive':     '🐑',
      'maniac':            '💣',
      'abc':               '📖',
      'short_stack_ninja': '🥷',
      'joker':             '🤡',
    };
    const BOT_CONFIG_BASE = 'https://github.com/ImperialBower/pkcore/blob/main/data/bots/';
    const BOT_SPECIAL_URLS = {
      'joker': 'https://www.youtube.com/watch?v=dQw4w9WgXcQ',
    };
    function botConfigUrl(name) {
      return BOT_SPECIAL_URLS[name] ?? (BOT_CONFIG_BASE + name + '.yaml');
    }

    // ── Action callout state ──────────────────────────────────────────────────
    // Tracks each seat's most recent action label so renderTableVisuals can
    // restore them after WASM state refreshes wipe out prior SVG updates.
    const lastActions = {};
    let betState = null;          // game state captured when bet controls open

    // ── Lifetime P&L ──────────────────────────────────────────────────────────
    const STARTING_CHIPS = 10_000;
    let lifetimePnl = parseInt(localStorage.getItem('lifetimePnl') ?? '0', 10) || 0;
    let currentGameChips = STARTING_CHIPS;
    let pendingGameCommitted = false;
    // Tracks the all-in amount per seat across streets (bet resets to 0 on flop+).
    let allInAmounts = new Map(); // seatIdx → chips committed when going all-in
    let allInHandNumber = -1;     // hand number when allInAmounts was populated

    function commitCurrentGameToLifetime(chipsAtCommit = currentGameChips) {
      if (pendingGameCommitted) return;
      lifetimePnl += chipsAtCommit - STARTING_CHIPS;
      localStorage.setItem('lifetimePnl', String(lifetimePnl));
      pendingGameCommitted = true;
    }

    function beginNewGame() {
      commitCurrentGameToLifetime();
      pendingGameCommitted = false;
      currentGameChips = STARTING_CHIPS;
      resetHandsCompleted('play');
    }

    function renderPnlSlot(chipsForCalc = currentGameChips) {
      const liveDelta = pendingGameCommitted ? 0 : (chipsForCalc - STARTING_CHIPS);
      const total = lifetimePnl + liveDelta;
      const el = document.getElementById('sc-pnl');
      el.textContent = (total >= 0 ? '+' : '') + '$' + total.toLocaleString();
      el.className = total > 0 ? 'positive' : total < 0 ? 'negative' : '';
    }
    renderPnlSlot(STARTING_CHIPS);

    // States that mean the player is NOT actively contesting the current hand.
    // Mirrors the Rust `is_in_hand()` helper (src/lib.rs:860) inverted.
    const HERO_NOT_IN_HAND_STATES = new Set(['Fold', 'Ready', 'Out']);

    // Enables the New Table button when it's safe to walk away — i.e. the
    // player is not actively in a hand. Called everywhere renderPnlSlot is.
    function updateNewTableButton(state) {
      const btn = document.getElementById('new-table-btn');
      if (!btn) return;
      const phase = state?.phase;
      const heroState = state?.hero?.state;
      const heroNotInHand = !heroState || HERO_NOT_IN_HAND_STATES.has(heroState);
      // Phase override: HandComplete = hand resolved (cards revealed, pots paid),
      // safe even if hero.state is still 'Showdown'. Uninitialized/SessionOver/Error
      // = no live hand to abandon.
      const phaseAllows = phase === 'Uninitialized'
                       || phase === 'HandComplete'
                       || phase === 'SessionOver'
                       || phase === 'Error';
      // Stack-zero escape hatch: if the player has no chips left (all-in or
      // busted), let them walk away immediately rather than waiting for the
      // hand to resolve. Trade-off: walking away while all-in forfeits any
      // potential pot winnings — the banked stack is whatever shows now ($0).
      const heroOutOfMoney = (state?.hero?.chips ?? null) === 0;
      btn.disabled = !(heroNotInHand || phaseAllows || heroOutOfMoney);
    }

    // ── Action callout helpers ────────────────────────────────────────────────

    // Shows (or hides) the action pill for a seat and persists the label. The
    // pill variant/colour mapping now lives in table.js (pillVariant).
    function setActionLabel(seat, label) {
      lastActions[seat] = label ?? null;
      table.setActionLabel(seat, label ?? null);
    }

    // Clears all action pills — called at the start of each new hand.
    function clearAllActions() {
      for (let i = 0; i < 9; i++) {
        lastActions[i] = null;
        table.setActionLabel(i, null);
      }
    }

    // Resets the table visuals + score bar to a blank pre-game state.
    function clearTable() {
      table.clear();
      clearAllActions();
      hideHandResult();
      document.getElementById('hero-cards').replaceChildren();
      document.getElementById('sc-hand').textContent   = '—';
      document.getElementById('sc-blinds').textContent = '—';
      document.getElementById('sc-chips').textContent  = '—';
      renderPnlSlot(STARTING_CHIPS);
      updateNewTableButton(null);
      setStatus('');
      hideBetControls();
    }

    // Shows the hand-result banner above the board.
    // isWin: true = won (accent), false/null = neutral.
    function showHandResult(text, isWin) {
      const el = document.getElementById('hand-result-overlay');
      if (!el) return;
      el.textContent = text;
      el.classList.toggle('win', !!isWin);
      el.hidden = false;
    }

    function hideHandResult() {
      const el = document.getElementById('hand-result-overlay');
      if (el) el.hidden = true;
    }

    // Builds a readable action label for the hero's action.
    function heroActionLabel(action, amount, state) {
      switch (action) {
        case 'Fold':  return 'folds';
        case 'Check': return 'checks';
        case 'Call':  return 'calls $' + (state?.to_call ?? amount).toLocaleString();
        case 'Bet':   return 'bets $' + amount.toLocaleString();
        case 'Raise': return 'raises $' + amount.toLocaleString();
        case 'AllIn': return 'all-in $' + amount.toLocaleString();
        default: return action.toLowerCase();
      }
    }

    // ── Bot animation loop ────────────────────────────────────────────────────
    // Calls step_bot() once per second until it's the human's turn or hand ends,
    // showing each bot's action in the status bar and as a graphical callout.
    async function stepBotsUntilHuman() {
      playLoopRunning = true;
      while (playLoopRunning) {
        const result = JSON.parse(_playMod.step_bot());
        if (result.done) {
          const state = JSON.parse(_playMod.get_state());
          playAdapter?.poke();  // pick up showdown / hand_end state changes
          renderState(state);
          return;
        }
        // Push bot action directly (pkcore doesn't expose action_log in get_state)
        if (result.seat != null && result.action_label) {
          const { verb, amount } = parseBotActionLabel(result.action_label);
          playAdapter?.pushEvent('action', result.seat, {
            verb, amount, pot_after: 0, to_call_after: 0,
          });
        }
        setStatus(`${result.name} ${result.action_label}`);
        // Opponents' hole cards stay hidden during play — they are revealed only
        // at showdown (see the showdown reveal block in the HandComplete handler).
        appendHandLog(`${result.name}: ${result.action_label}`);
        setActionLabel(result.seat, result.action_label);
        const state = JSON.parse(_playMod.get_state());
        renderTableVisuals(state);
        await new Promise(r => setTimeout(r, BOT_ACTION_MS));
      }
    }

    // ── Boot ──────────────────────────────────────────────────────────────────

    // ── URL state persistence ──────────────────────────────────────────────────

    function updateUrlState(mode, handNumber) {
      history.replaceState(null, '', `#mode=${mode}&seed=${gameSeed}&hand=${handNumber}`);
    }

    function parseUrlHash() {
      const h = location.hash.slice(1);
      if (!h) return null;
      const params = Object.fromEntries(h.split('&').map(p => p.split('=')));
      const seed = parseFloat(params.seed);
      const hand = parseInt(params.hand, 10);
      const mode = params.mode;
      if (!mode || isNaN(seed) || isNaN(hand) || hand < 1) return null;
      return { mode, seed, hand };
    }

    async function fastForwardToHand(targetHand) {
      const savedBot  = BOT_ACTION_MS;
      const savedHand = HAND_COMPLETE_MS;
      BOT_ACTION_MS   = 0;
      HAND_COMPLETE_MS = 0;
      arenaRunning = true;

      while (arenaRunning) {
        while (true) {
          const result = JSON.parse(_arenaMod.step_bot());
          if (result.done) break;
        }
        const state = JSON.parse(_arenaMod.get_state());
        renderTableVisuals(state);
        updateArenaStatus(state);

        if (state.phase === 'SessionOver') {
          arenaRunning = false;
          document.getElementById('arena-start-btn').textContent = 'New Arena';
          BOT_ACTION_MS   = savedBot;
          HAND_COMPLETE_MS = savedHand;
          return;
        }
        if (state.hand_number >= targetHand) break;
        noteHandCompleted('arena');
        _arenaMod.next_hand();
        // yield to browser between hands so the tab stays responsive
        await new Promise(r => setTimeout(r, 0));
      }

      BOT_ACTION_MS   = savedBot;
      HAND_COMPLETE_MS = savedHand;
      runArena();
    }

    async function restoreFromUrl() {
      const saved = parseUrlHash();
      if (!saved) return false;

      gameSeed = saved.seed;
      document.getElementById('btn-new-game').disabled = false;

      if (saved.mode === 'arena') {
        activateTab('arena');                                         // UI only — WASM not ready yet
        const state = JSON.parse(_arenaMod.init_bot_game(gameSeed)); // init WASM first
        renderTableVisuals(state);
        updateArenaStatus(state);
        document.getElementById('arena-start-btn').textContent = 'New Arena';

        if (saved.hand > 1) {
          setStatus(`Restoring arena to hand #${saved.hand}…`);
          await fastForwardToHand(saved.hand);
        } else {
          runArena();
        }
      } else {
        // Play: restore same bot lineup; human actions aren't saved so restart from hand 1
        activateTab('play');                                     // UI only — WASM not ready yet
        playBlindLevel = 0;
        const state = JSON.parse(_playMod.init_game(gameSeed)); // init WASM first
        renderTableVisuals(state);
        setStatus('Resumed — same opponents, starting from hand 1.');
        enableCurrentGameButton();
        stepBotsUntilHuman();
      }
      return true;
    }

    async function boot() {
      try {
        [_playMod, _arenaMod] = await Promise.all([
          import('../pkg/pkarena0_web.js?tab=play').then(async m  => { await m.default(); return m; }),
          import('../pkg/pkarena0_web.js?tab=arena').then(async m => { await m.default(); return m; }),
        ]);
        document.getElementById('sc-version').textContent = 'v' + _playMod.version();
        const restored = await restoreFromUrl();
        if (!restored) {
          setStatus('Click "New Game" to start.');
          document.getElementById('btn-new-game').disabled = false;
        }
      } catch (e) {
        setStatus('Failed to load WASM: ' + e);
      }
    }
    initThemes();
    initDeckToggle();
    initViewToggle();
    boot();

    // ── Audio ─────────────────────────────────────────────────────────────────

    // Must be called inside a user-gesture handler to satisfy AudioContext rules.
    function ensureVoice() {
      if (voice) return;
      voice = new Voice({ basePath: './audio/voice/' });
      voice.preload().then(({ loaded, missing }) =>
        console.log(`[audio] ${loaded} clips loaded, ${missing} missing`));
      window.voice = voice;
    }

    function activatePlayAudio() {
      if (!voice || !_playMod) return;
      arenaAdapter?.stop();
      arenaAdapter = null;
      playAdapter?.stop();
      playAdapter = new LiveAdapter({
        getState: () => JSON.parse(_playMod.get_state()),
        onEvent:  handleEvent,
        intervalMs: 150,
      });
      playAdapter.start();
      window.playAdapter = playAdapter;
    }

    function activateArenaAudio() {
      if (!voice || !_arenaMod) return;
      playAdapter?.stop();
      playAdapter = null;
      arenaAdapter?.stop();
      arenaAdapter = new LiveAdapter({
        getState: () => JSON.parse(_arenaMod.get_state()),
        onEvent:  handleEvent,
        intervalMs: 150,
      });
      arenaAdapter.start();
      window.arenaAdapter = arenaAdapter;
    }

    // Parse a bot action_label string ("raises $400", "folds", etc.) into
    // a verb key and numeric amount that voice.js understands.
    function parseBotActionLabel(label) {
      const s = (label ?? '').toLowerCase().trim();
      const amt = m => parseInt((m[1] ?? '0').replace(/,/g, ''), 10) || 0;
      if (s === 'checks' || s === 'check') return { verb: 'check', amount: 0 };
      if (s === 'folds'  || s === 'fold')  return { verb: 'fold',  amount: 0 };
      if (s === 'mucks'  || s === 'muck')  return { verb: 'muck',  amount: 0 };
      let m;
      if ((m = s.match(/^calls?\s+\$?([\d,]+)/)))  return { verb: 'call',  amount: amt(m) };
      if ((m = s.match(/^bets?\s+\$?([\d,]+)/)))   return { verb: 'bet',   amount: amt(m) };
      if ((m = s.match(/^raises?\s+\$?([\d,]+)/))) return { verb: 'raise', amount: amt(m) };
      if ((m = s.match(/^all.?in\s+\$?([\d,]+)/)))return { verb: 'allin', amount: amt(m) };
      if (s.includes('small blind')) return { verb: 'post_sb', amount: 0 };
      if (s.includes('big blind'))   return { verb: 'post_bb', amount: 0 };
      return { verb: s, amount: 0 };
    }

    // Map human action enum ('Fold','Check','Call','Bet','Raise','AllIn') to voice verb.
    function heroVerbToVoiceVerb(action) {
      const map = { Fold: 'fold', Check: 'check', Call: 'call',
                    Bet: 'bet', Raise: 'raise', AllIn: 'allin' };
      return map[action] ?? action.toLowerCase();
    }

    function handleEvent(ev) {
      if (!voice || !audioEnabled) return;
      console.debug('[audio]', ev.kind, ev.seat ?? '', ev.data);
      switch (ev.kind) {
        case 'hand_start':
          voice.cancel();
          break;
        case 'deal':
          if (ev.data.to === 'hero') voice.say.yourHand(ev.data.cards);
          break;
        case 'street':
          if (ev.data.street === 'flop')       voice.say.flop(ev.data.board);
          else if (ev.data.street === 'turn')  voice.say.turn(ev.data.board[3]);
          else if (ev.data.street === 'river') voice.say.river(ev.data.board[4]);
          break;
        case 'action':
          voice.say.seatAction(ev.seat, ev.data.verb, ev.data.amount);
          break;
        case 'your_turn':
          voice.say.yourTurn({ toCall: ev.data.to_call, pot: ev.data.pot, stack: ev.data.stack });
          break;
        case 'showdown': {
          const isChop = ev.data.winners.length > 1;
          if (isChop) {
            voice.say.line('result_chop');
          } else if (ev.data.winners[0]) {
            const w = ev.data.winners[0];
            voice.say.showdownWin(w.seat, []).then(() => {
              voice.say.line(w.seat === 0 ? 'result_you_win' : 'result_you_lose');
            });
          }
          break;
        }
      }
    }

    // ── New game ──────────────────────────────────────────────────────────────
    function startNewGame() {
      beginNewGame();
      ensureVoice();
      hideBetControls();
      clearAllActions();
      hideHandResult();
      playBlindLevel = 0;
      gameSeed = Math.random();
      const state = JSON.parse(_playMod.init_game(gameSeed));
      updateUrlState('play', 1);
      renderTableVisuals(state);
      setStatus('Dealing…');
      enableCurrentGameButton();
      activatePlayAudio();  // creates fresh adapter; first tick fires hand_start + deal events
      stepBotsUntilHuman();
    }

    function showNewGameButton() {
      const container = document.getElementById('action-buttons');
      while (container.firstChild) container.removeChild(container.firstChild);
      const btn = document.createElement('button');
      btn.id = 'btn-new-game';
      btn.className = 'primary';
      btn.textContent = 'New Game';
      btn.addEventListener('click', startNewGame);
      container.appendChild(btn);
    }

    document.getElementById('btn-new-game').addEventListener('click', startNewGame);

    // ── Human action ─────────────────────────────────────────────────────────
    function onHumanAction(action, amount, currentState) {
      playAdapter?.pushEvent('action', 0, {
        verb: heroVerbToVoiceVerb(action), amount: amount ?? 0,
        pot_after: 0, to_call_after: 0,
      });
      hideBetControls();
      renderButtons([], null);   // clear buttons immediately so none appear while bots act
      const label = heroActionLabel(action, amount, currentState);
      setActionLabel(0, label);
      const heroCards = cardsToLogStr(currentState.hero?.hole_cards);
      appendHandLog(`You${heroCards ? ' ' + heroCards : ''}: ${label}`);
      setStatus('…');
      const req = JSON.stringify({ action, amount: amount ?? 0 });
      const state = JSON.parse(_playMod.human_action(req));
      // Legacy hard-error path (should not occur after the derive_legal_actions fix).
      if (state.phase === 'Error') { renderState(state); return; }
      // Recoverable error: action was rejected but game is still WaitingForHuman.
      // Show the message and re-render buttons so the player can try again.
      if (state.error) {
        setStatus('Error: ' + state.error);
        renderTableVisuals(state);
        renderActionButtons(state);
        return;
      }
      playBlindLevel = syncBlindLevel(_playMod, state.hand_number, playBlindLevel);
      renderTableVisuals(state);
      stepBotsUntilHuman();
    }

    // ── Render ────────────────────────────────────────────────────────────────

    // Maps the WASM GameState JSON onto the shape table.update() expects.
    function buildTableView(state) {
      return {
        pot: state.pot ?? 0,
        board: state.board ?? [],
        smallBlind: state.small_blind ?? 0,
        bigBlind: state.big_blind ?? 0,
        seats: [state.hero, ...(state.players ?? [])].filter(Boolean),
        dealerSeat: state.dealer_seat ?? null,
        currentSeat: null,
        heroSeat: 0,
        heroTurn: state.phase === 'WaitingForHuman',
        allInAmounts,
        showHud: false,
        nameHref: botConfigUrl,
        emoji: name => BOT_EMOJIS[name] ?? '',
      };
    }

    // Updates the table + score bar; does NOT touch status or action buttons.
    function renderTableVisuals(state) {
      if (!state || state.phase === 'Error') return;

      const chips = state.hero?.chips ?? 0;
      currentGameChips = chips;
      document.getElementById('sc-hand').textContent = state.hand_number ?? '—';
      document.getElementById('sc-blinds').textContent =
        state.small_blind ? `${state.small_blind}/${state.big_blind}` : '—';
      document.getElementById('sc-chips').textContent = '$' + chips.toLocaleString();
      renderPnlSlot();
      updateNewTableButton(state);

      // All-in amount tracking: persists across streets (bet resets to 0 postflop).
      if (state.hand_number !== allInHandNumber) {
        allInAmounts.clear();
        allInHandNumber = state.hand_number;
      }
      for (const p of [state.hero, ...(state.players ?? [])].filter(Boolean)) {
        if (p.state === 'AllIn') {
          if (p.bet > 0) allInAmounts.set(p.seat, p.bet);
        } else {
          allInAmounts.delete(p.seat);
        }
      }

      table.update(buildTableView(state));
      updateHeroCardsDisplay(state.hero);
      renderHeroDock(state);
    }

    const POS_TAGS = ['BTN','SB','BB','UTG','UTG+1','MP','LJ','HJ','CO'];
    function renderHeroDock(state) {
      const hero = state.hero;
      const title = document.getElementById('hero-title');
      const sub = document.getElementById('hero-sub');
      if (!hero) { title.textContent = ''; sub.textContent = ''; return; }
      const chips = hero.chips ?? 0;
      const bb = state.big_blind > 0 ? Math.round(chips / state.big_blind) : 0;
      const pos = state.dealer_seat != null
        ? POS_TAGS[((0 - state.dealer_seat) % 9 + 9) % 9] : '';
      title.textContent = `You${pos ? ' · ' + pos : ''} · $${chips.toLocaleString()} (${bb} BB)`;
      const toCall = state.to_call ?? 0;
      // pkcore's `pot` excludes live (uncommitted) street bets, so fold them back
      // in for realistic pot-odds/SPR — otherwise preflop reads POT ODDS 100%.
      const liveBets = [state.hero, ...(state.players ?? [])]
        .filter(Boolean).reduce((s, p) => s + (p.bet ?? 0), 0);
      const pot = (state.pot ?? 0) + liveBets;
      const odds = toCall > 0 ? Math.round((toCall / (pot + toCall)) * 100) : 0;
      const spr = pot > 0 ? (chips / pot).toFixed(1) : '—';
      const hand = cardsToLogStr(hero.hole_cards);
      sub.textContent =
        `${hand ? hand + ' · ' : ''}TO CALL $${toCall.toLocaleString()} · POT ODDS ${odds}% · SPR ${spr}`;
    }

    // Full render: visuals + phase-appropriate status and action buttons.
    function renderState(state) {
      if (!state || state.phase === 'Error') {
        setStatus('Error: ' + (state?.error ?? 'unknown'));
        updateNewTableButton(state);
        return;
      }

      renderTableVisuals(state);
      const chips = state.hero?.chips ?? 0;

      if (state.phase === 'SessionOver') {
        commitCurrentGameToLifetime(chips);
        renderPnlSlot();
        updateNewTableButton(state);
        setStatus('Session over! Final chips: $' + chips.toLocaleString());
        renderButtons([{ label: 'New Game', cls: 'primary', action: 'new-game' }]);
        appendHandLog('Session ended. Final chips: $' + chips.toLocaleString());
        return;
      }

      if (state.phase === 'HandComplete') {
        const street = state.street ?? '';
        appendHandLog('Hand #' + state.hand_number + ' complete — ' + street);
        enableYamlDownload();
        noteHandCompleted('play');
        renderButtons([]);

        // Advance the hand (syncs blind level, then calls next_hand()).
        const { state: nextState, blindLevel: newPlayLevel } = advanceHand(_playMod, state.hand_number, playBlindLevel);
        playBlindLevel = newPlayLevel;
        if (nextState.phase === 'Error') {
          console.error('[pkarena] next_hand() failed:', nextState.error);
        }
        const heroChipsAfter = nextState.hero?.chips ?? state.hero?.chips ?? 0;

        // Immediately refresh chip labels so the score bar shows settled winnings
        // rather than $0 when the hero was all-in when the hand ended.
        {
          document.getElementById('sc-chips').textContent = '$' + heroChipsAfter.toLocaleString();
          currentGameChips = heroChipsAfter;
          renderPnlSlot();
          // The user perceives this moment as "between hands" — keep button enabled
          // even though WASM has internally advanced phase to BotsActing.
          updateNewTableButton({ phase: 'HandComplete' });
          setText('seat-0-chips', '$' + heroChipsAfter.toLocaleString());
          for (const p of nextState.players ?? []) {
            setText('seat-' + p.seat + '-chips', '$' + (p.chips ?? 0).toLocaleString());
          }
        }

        // Build the result message from the engine's winner summary.
        const result = nextState.last_result?.[0];
        let resultText, isWin = null;
        if (result) {
          const heroWon = result.seats.includes(0);
          const displayNames = result.names.map(n => n === 'You' ? 'You' : n);
          const isSplit = displayNames.length > 1;
          const winnerStr = isSplit
            ? displayNames.slice(0, -1).join(', ') + ' & ' + displayNames.at(-1)
            : (displayNames[0] ?? 'Unknown');
          const verb = isSplit ? 'split' : 'won';
          const handStr = result.hand ? ` — ${result.hand}` : '';
          resultText = `${winnerStr} ${verb} $${result.amount.toLocaleString()}${handStr}`;
          isWin = heroWon;
        }
        if (resultText) showHandResult(resultText, isWin);

        // ── Persist the outcome to the hand log (scrollback), not just banner ──
        const showdown = nextState.showdown;
        if (Array.isArray(showdown) && showdown.length) {
          // Winner seats + amount won per seat, summed across pots (side pots),
          // splitting an entry's amount across its seats for chopped pots.
          const wonBySeat = new Map();
          for (const pot of nextState.last_result ?? []) {
            const n = pot.seats.length || 1;
            const base = Math.floor(pot.amount / n);
            let rem = pot.amount - base * n; // odd chips: hand one each to the first `rem` seats
            for (const s of pot.seats) {
              wonBySeat.set(s, (wonBySeat.get(s) ?? 0) + base + (rem > 0 ? 1 : 0));
              if (rem > 0) rem--;
            }
          }
          // Winners first, then the rest.
          const ordered = [...showdown].sort(
            (a, b) => (wonBySeat.has(b.seat) ? 1 : 0) - (wonBySeat.has(a.seat) ? 1 : 0),
          );
          for (const p of ordered) {
            const name = p.seat === 0 ? 'You' : p.name;
            const cards = cardsToLogStr(p.cards);
            const catStr = p.hand ? `: ${p.hand}` : ''; // omit the colon when category is unknown
            if (wonBySeat.has(p.seat)) {
              const amt = wonBySeat.get(p.seat); // already an integer (exact chip distribution above)
              appendHandLog(`★ ${name} ${cards}${catStr} — wins $${amt.toLocaleString()}`);
            } else {
              appendHandLog(`  ${name} ${cards}${catStr}`);
            }
          }
        } else if (result) {
          // Fold-out: one uncontested winner. No hand category (single-seat eval
          // is meaningless). The winner's own fold/action line is already above.
          const displayNames = result.names.map(n => (n === 'You' ? 'You' : n));
          const winnerStr = displayNames[0] ?? 'Unknown';
          const verb = winnerStr === 'You' ? 'win' : 'wins';
          appendHandLog(`${winnerStr} ${verb} $${result.amount.toLocaleString()} uncontested`);
        }

        // When the session is over, say so immediately in the status bar.
        if (nextState.phase === 'SessionOver') {
          setStatus(resultText ? resultText + ' — Session over!' : 'Session over!');
        } else if (nextState.phase === 'Error') {
          setStatus('Engine error — see details shortly…');
        } else if (resultText) {
          setStatus(resultText);
        } else if (nextState.error) {
          setStatus(nextState.error);
        }

        setTimeout(() => {
          if (document.body.dataset.tab !== 'play') return; // tab was switched away
          hideHandResult();
          clearAllActions();
          if (nextState.phase === 'SessionOver') { renderState(nextState); return; }
          if (nextState.phase === 'Error') {
            setStatus('Engine error: ' + (nextState.error ?? 'unknown'));
            renderButtons([{ label: 'New Game', cls: 'primary', action: 'new-game' }]);
            return;
          }
          updateUrlState('play', nextState.hand_number);
          renderTableVisuals(nextState);
          stepBotsUntilHuman();
        }, HAND_COMPLETE_MS);
        return;
      }

      // WaitingForHuman
      const street = state.street ?? 'Preflop';
      setStatus('Hand #' + state.hand_number + ' — ' + street + ' — Your turn.');
      renderActionButtons(state);
    }

    // ── Hero hole cards (large, in the dock) ──────────────────────────────────
    function updateHeroCardsDisplay(hero) {
      const el = document.getElementById('hero-cards');
      if (!el) return;
      el.replaceChildren();
      const cards = hero?.hole_cards;
      if (!cards || cards.length < 2 || !cards[0]) return;
      cards.forEach(c => { const node = makeCard(c, 'hero'); if (node) el.appendChild(node); });
    }

    // ── Action buttons ────────────────────────────────────────────────────────
    let pendingBetAction = 'Bet';

    function renderActionButtons(state) {
      const actions = state.legal_actions ?? [];
      const btns = [];

      for (const action of actions) {
        if (action === 'Fold') {
          btns.push({ label: 'Fold', cls: 'danger', action });
        } else if (action === 'Check') {
          btns.push({ label: 'Check', cls: 'safe', action });
        } else if (action === 'Call') {
          const amt = state.to_call ?? 0;
          btns.push({ label: 'Call $' + amt.toLocaleString(), cls: '', action });
        } else if (action === 'Bet' || action === 'Raise') {
          const minRaise = state.min_raise ?? 100;
          const maxBet   = state.max_bet   ?? 10000;
          const pot      = state.pot       ?? 0;
          // Min raise — always shown
          btns.push({ label: 'Min $' + minRaise.toLocaleString(), cls: 'primary', action: 'bet-direct', betAction: action, amount: minRaise });
          // ½ pot — shown when it's meaningfully above min and below max
          const halfPot = Math.round(pot / 2);
          if (halfPot > minRaise && halfPot < maxBet)
            btns.push({ label: '½ Pot $' + halfPot.toLocaleString(), cls: 'primary', action: 'bet-direct', betAction: action, amount: halfPot });
          // Full pot — shown when it's above min and below max (all-in handles the rest)
          if (pot > minRaise && pot < maxBet)
            btns.push({ label: 'Pot $' + pot.toLocaleString(), cls: 'primary', action: 'bet-direct', betAction: action, amount: pot });
          // Custom slider
          btns.push({ label: action + '…', cls: 'primary', action: 'bet-open', betAction: action });
        } else if (action === 'AllIn') {
          const amt = state.max_bet ?? 0;
          btns.push({ label: 'All-In $' + amt.toLocaleString(), cls: 'danger', action: 'allin', amount: amt });
        }
      }

      renderButtons(btns, state);
    }

    function renderButtons(btns, state) {
      const container = document.getElementById('action-buttons');
      while (container.firstChild) container.removeChild(container.firstChild);

      for (const b of btns) {
        const btn = document.createElement('button');
        btn.textContent = b.label;
        if (b.cls) btn.className = b.cls;

        btn.dataset.act =
          b.betAction === 'raise-open' || /raise/i.test(b.label) ? 'raise'
          : /min/i.test(b.label) ? 'min'
          : /all-?in/i.test(b.label) ? 'allin'
          : /call/i.test(b.label) ? 'call'
          : /check/i.test(b.label) ? 'check'
          : /bet/i.test(b.label) ? 'bet'
          : /fold/i.test(b.label) ? 'fold' : '';

        if (b.action === 'new-game') {
          btn.addEventListener('click', () => {
            beginNewGame();
            hideBetControls();
            clearAllActions();
            hideHandResult();
            playBlindLevel = 0;
            gameSeed = Math.random();
            const s = JSON.parse(_playMod.init_game(gameSeed));
            updateUrlState('play', 1);
            renderTableVisuals(s);
            setStatus('Dealing…');
            enableCurrentGameButton();
            stepBotsUntilHuman();
          });
        } else if (b.action === 'bet-open') {
          btn.addEventListener('click', () => {
            pendingBetAction = b.betAction;
            showBetControls(state);
          });
        } else if (b.action === 'bet-direct') {
          btn.addEventListener('click', () => onHumanAction(b.betAction, b.amount, state));
        } else if (b.action === 'allin') {
          btn.addEventListener('click', () => onHumanAction('AllIn', b.amount, state));
        } else {
          btn.addEventListener('click', () => onHumanAction(b.action, 0, state));
        }
        container.appendChild(btn);
      }
    }

    // ── Raise strip ───────────────────────────────────────────────────────────
    function setRaiseAmount(v) {
      const slider = document.getElementById('bet-slider');
      const input = document.getElementById('bet-input');
      const clamped = Math.max(Number(slider.min), Math.min(Number(slider.max), Math.round(v)));
      slider.value = String(clamped);
      input.value = String(clamped);
      document.getElementById('raise-amount').textContent = '$' + clamped.toLocaleString();
    }

    function showBetControls(state) {
      pendingBetAction = state.legal_actions?.includes('Raise') ? 'Raise' : 'Bet';
      betState = state;
      const slider = document.getElementById('bet-slider');
      const min = state.min_raise ?? state.big_blind ?? 0;
      const max = state.max_bet ?? (state.hero?.chips ?? 0);
      slider.min = String(min);
      slider.max = String(max);
      slider.step = String(state.big_blind || 50);
      document.getElementById('raise-strip').hidden = false;
      setRaiseAmount(min);
    }

    function hideBetControls() {
      document.getElementById('raise-strip').hidden = true;
    }

    document.getElementById('bet-slider').addEventListener('input', function () { setRaiseAmount(Number(this.value)); });
    document.getElementById('bet-input').addEventListener('input', function () { setRaiseAmount(Number(this.value)); });
    document.getElementById('raise-min').addEventListener('click', () => setRaiseAmount(Number(betState?.min_raise ?? betState?.big_blind ?? 0)));
    document.getElementById('raise-3x').addEventListener('click', () => setRaiseAmount(3 * (betState?.big_blind ?? 0)));
    document.getElementById('raise-pot').addEventListener('click', () => setRaiseAmount(betState?.pot ?? 0));
    document.getElementById('raise-allin').addEventListener('click', () => setRaiseAmount(Number(betState?.max_bet ?? betState?.hero?.chips ?? 0)));
    document.getElementById('bet-confirm').addEventListener('click', () => {
      const amount = parseInt(document.getElementById('bet-input').value, 10);
      if (!isNaN(amount)) onHumanAction(pendingBetAction, amount, betState);
    });

    // ── Utilities ─────────────────────────────────────────────────────────────
    function setText(id, value) {
      const el = document.getElementById(id);
      if (el) el.textContent = value;
    }

    function setStatus(msg) {
      document.getElementById('status-msg').textContent = msg;
    }

    function disableActionButtons() {
      const btns = document.querySelectorAll('#action-buttons button');
      btns.forEach(b => b.disabled = true);
    }

    function appendHandLog(entry) {
      const log = document.getElementById('hand-log');
      const p = document.createElement('p');
      p.textContent = entry;
      log.appendChild(p);
      log.scrollTop = log.scrollHeight;
    }

    // Convert a two-char ASCII card code ("Ks", "Td", "Ah") to a unicode string ("K♠", "10♦", "A♥").
    function cardToLogStr(c) {
      if (!c || c === '__') return '?';
      const suit = c[c.length - 1];
      const rank = c.slice(0, -1);
      const suitSym = { s: '♠', h: '♥', d: '♦', c: '♣' }[suit] ?? suit;
      return (rank === 'T' ? '10' : rank) + suitSym;
    }

    // Format an array of card codes as "[K♠ Q♥]", or "" if empty/absent.
    function cardsToLogStr(cards) {
      if (!cards || cards.length === 0) return '';
      return '[' + cards.map(cardToLogStr).join(' ') + ']';
    }

    // ── YAML download ─────────────────────────────────────────────────────────
    document.getElementById('btn-download-yaml').addEventListener('click', () => {
      const mod = document.body.dataset.tab === 'arena' ? _arenaMod : _playMod;
      const yaml = mod.get_session_yaml();
      const blob = new Blob([yaml], { type: 'text/yaml' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'pkarena0-session.yaml';
      a.click();
      URL.revokeObjectURL(url);
    });

    // ── Current game state debug overlay ──────────────────────────────────────
    const gameStateOverlay = document.getElementById('game-state-overlay');
    const gameStatePre     = document.getElementById('game-state-pre');

    document.getElementById('btn-current-game').addEventListener('click', () => {
      const mod = document.body.dataset.tab === 'arena' ? _arenaMod : _playMod;
      const raw = mod.get_state();
      try {
        gameStatePre.textContent = JSON.stringify(JSON.parse(raw), null, 2);
      } catch {
        gameStatePre.textContent = raw;
      }
      gameStateOverlay.classList.add('open');
    });

    // Settings → "View game state (JSON)": populate + open the debug overlay.
    document.getElementById('settings-view-state').addEventListener('click', () => {
      const mod = document.body.dataset.tab === 'arena' ? _arenaMod : _playMod;
      if (mod) {
        const raw = mod.get_state();
        try { gameStatePre.textContent = JSON.stringify(JSON.parse(raw), null, 2); }
        catch { gameStatePre.textContent = raw; }
      }
      settingsOverlay.classList.remove('open');
      gameStateOverlay.classList.add('open');
    });

    document.getElementById('game-state-close').addEventListener('click', () => {
      gameStateOverlay.classList.remove('open');
    });

    gameStateOverlay.addEventListener('click', (e) => {
      if (e.target === gameStateOverlay) gameStateOverlay.classList.remove('open');
    });

    // ── Hand-log aside / mobile bottom drawer ──────────────────────────────────
    const logAside = document.getElementById('log-aside');
    const logBackdrop = document.getElementById('log-backdrop');
    function setLog(open) {
      logAside.hidden = !open;
      logBackdrop.hidden = !open;   // inert on desktop (backdrop is display:none there)
    }
    document.getElementById('log-toggle').addEventListener('click', () => setLog(logAside.hidden));
    document.getElementById('log-close').addEventListener('click', () => setLog(false));
    logBackdrop.addEventListener('click', () => setLog(false));

    // ── Settings overlay ───────────────────────────────────────────────────────
    const settingsOverlay = document.getElementById('settings-overlay');

    document.getElementById('settings-btn').addEventListener('click', () => {
      settingsOverlay.classList.add('open');
    });

    document.getElementById('settings-close').addEventListener('click', () => {
      settingsOverlay.classList.remove('open');
    });

    settingsOverlay.addEventListener('click', (e) => {
      if (e.target === settingsOverlay) settingsOverlay.classList.remove('open');
    });

    // ── Replay button tally (overlay state + listeners live in replay.js) ─────

    // Per-tab tally of completed hands; the replay button is enabled when the
    // currently-active tab has at least one completed hand to replay.
    const handsCompleted = { play: 0, arena: 0 };

    function updateReplayButtonState() {
      const tab = document.body.dataset.tab;
      const count = handsCompleted[tab] ?? 0;
      document.getElementById('btn-replay').disabled = count === 0;
    }

    function noteHandCompleted(tab) {
      handsCompleted[tab] = (handsCompleted[tab] ?? 0) + 1;
      updateReplayButtonState();
    }

    function resetHandsCompleted(tab) {
      handsCompleted[tab] = 0;
      updateReplayButtonState();
    }

    function activeWasmMod() {
      return document.body.dataset.tab === 'arena' ? _arenaMod : _playMod;
    }

    // Replay overlay: state, snapshot rendering and all #replay-* listeners
    // live in replay.js; it renders through its own createTable instance.
    initReplay({ getMod: activeWasmMod });

    const BLIND_LEVELS = [
      { sb:   50, bb:   100, hands: 10 },
      { sb:  100, bb:   200, hands: 10 },
      { sb:  150, bb:   300, hands: 10 },
      { sb:  200, bb:   400, hands: 10 },
      { sb:  300, bb:   600, hands: 10 },
      { sb:  400, bb:   800, hands: 10 },
      { sb:  500, bb:  1000, hands: 10 },
      { sb:  750, bb:  1500, hands: 10 },
      { sb: 1000, bb:  2000, hands: 10 },
      { sb: 1500, bb:  3000, hands: 10 },
      { sb: 2000, bb:  4000, hands: 10 },
      { sb: 3000, bb:  6000, hands: 10 },
    ];

    function getBlindLevelForHand(handNumber) {
      let hands = 0;
      for (let i = 0; i < BLIND_LEVELS.length - 1; i++) {
        hands += BLIND_LEVELS[i].hands;
        if (handNumber <= hands) return i;
      }
      return BLIND_LEVELS.length - 1;
    }

    function syncBlindLevel(mod, handNumber, currentLevel) {
      const newLevel = getBlindLevelForHand(handNumber);
      if (newLevel !== currentLevel) {
        const { sb, bb } = BLIND_LEVELS[newLevel];
        mod.set_blinds(sb, bb);
      }
      return newLevel;
    }

    // Sync blinds for the incoming hand, then start it. Returns the new GameState.
    function advanceHand(mod, currentHandNumber, blindLevel) {
      const newLevel = syncBlindLevel(mod, currentHandNumber + 1, blindLevel);
      const state = JSON.parse(mod.next_hand());
      return { state, blindLevel: newLevel };
    }

    const SPEED_PRESETS = [
      { label: 'Very Slow (0.25×)', bot: 4000, hand: 20000 },
      { label: 'Slow (0.5×)',       bot: 2000, hand: 10000 },
      { label: 'Slow (0.75×)',      bot: 1500, hand:  7500 },
      { label: 'Normal (1×)',       bot: 1000, hand:  5000 },
      { label: 'Fast (1.5×)',       bot:  700, hand:  3500 },
      { label: 'Fast (2×)',         bot:  500, hand:  2500 },
      { label: 'Fast (3×)',         bot:  350, hand:  1750 },
      { label: 'Fast (4×)',         bot:  250, hand:  1250 },
      { label: 'Very Fast (6×)',    bot:  150, hand:   800 },
      { label: 'Turbo (10×)',       bot:   75, hand:   400 },
    ];

    document.getElementById('speed-slider').addEventListener('input', (e) => {
      const preset = SPEED_PRESETS[e.target.value - 1];
      BOT_ACTION_MS = preset.bot;
      HAND_COMPLETE_MS = preset.hand;
      document.getElementById('speed-label').textContent = preset.label;
    });

    const audioToggleEl = document.getElementById('audio-toggle');
    audioToggleEl.checked = audioEnabled;
    audioToggleEl.addEventListener('change', () => {
      audioEnabled = audioToggleEl.checked;
      localStorage.setItem('audioEnabled', audioEnabled);
      if (!audioEnabled) voice?.cancel();
    });

    document.getElementById('reset-pnl-btn').addEventListener('click', () => {
      if (!confirm('Reset lifetime P&L to $0? This cannot be undone.')) return;
      lifetimePnl = 0;
      localStorage.setItem('lifetimePnl', '0');
      renderPnlSlot();
    });

    document.getElementById('new-table-btn').addEventListener('click', () => {
      const chips = currentGameChips;
      if (chips > STARTING_CHIPS) {
        const profit = chips - STARTING_CHIPS;
        if (!confirm('Walk away with $' + profit.toLocaleString() + ' profit? Your chips will be banked to lifetime P&L.')) return;
      }
      // Mirrors the inline 'new-game' action handler in renderButtons().
      beginNewGame();
      hideBetControls();
      clearAllActions();
      hideHandResult();
      playBlindLevel = 0;
      gameSeed = Math.random();
      const s = JSON.parse(_playMod.init_game(gameSeed));
      updateUrlState('play', 1);
      renderTableVisuals(s);
      setStatus('New table — fresh bots dealing in…');
      enableCurrentGameButton();
      stepBotsUntilHuman();
    });

    const audioStatusEl = document.getElementById('audio-status');
    document.getElementById('audio-test-btn').addEventListener('click', () => {
      audioStatusEl.textContent = 'testing…';
      // Test 1: raw SpeechSynthesis (no game required)
      if (!('speechSynthesis' in window)) {
        audioStatusEl.textContent = 'SpeechSynthesis not supported';
        return;
      }
      const u = new SpeechSynthesisUtterance('audio test');
      u.onstart = () => { audioStatusEl.textContent = 'TTS speaking ✓'; };
      u.onend   = () => { audioStatusEl.textContent = 'TTS ok — click New Game for full narration'; };
      u.onerror = (e) => { audioStatusEl.textContent = 'TTS error: ' + e.error; };
      speechSynthesis.speak(u);
    });

    // ── Game modes — add a new entry here to add a tab ──────────────────────
    const GAME_MODES = [
      { id: 'play',  label: 'Play' },
      { id: 'arena', label: 'Arena' },
    ];

    // UI-only tab switch — safe to call before WASM is initialized
    function activateTab(id) {
      document.querySelectorAll('.tab').forEach(t =>
        t.classList.toggle('active', t.dataset.tab === id));
      document.body.dataset.tab = id;
      updateReplayButtonState();
    }

    function restorePlayTabState() {
      if (!_playMod) return;
      let state;
      try { state = JSON.parse(_playMod.get_state()); } catch { state = null; }
      if (!state || !state.phase || state.phase === 'Uninitialized') {
        showNewGameButton();
        setStatus('Click "New Game" to start.');
        return;
      }
      renderTableVisuals(state);
      if (state.phase === 'WaitingForHuman') {
        renderState(state);
      } else if (state.phase === 'BotsActing') {
        setStatus('Resuming…');
        stepBotsUntilHuman();
      } else {
        renderState(state);
      }
    }

    function restoreArenaTabState() {
      if (!_arenaMod) return;
      let state;
      try { state = JSON.parse(_arenaMod.get_state()); } catch { state = null; }
      if (!state || !state.phase || state.phase === 'Uninitialized') {
        document.getElementById('arena-start-btn').textContent = 'Start Arena';
        document.getElementById('arena-status').textContent =
          'Press Start Arena to watch 9 bots play to the finish.';
        return;
      }
      renderTableVisuals(state);
      updateArenaStatus(state);
      if (state.phase !== 'SessionOver') {
        runArena(); // resume; arenaGeneration prevents any winding-down loop from conflicting
      }
    }

    function switchTab(id) {
      if (document.body.dataset.tab === id) return;
      arenaRunning = false;
      playLoopRunning = false;
      activateTab(id);
      if (id === 'play')  { restorePlayTabState();  if (voice) activatePlayAudio(); }
      if (id === 'arena') { restoreArenaTabState(); if (voice) activateArenaAudio(); }
    }

    // Build tab bar from GAME_MODES and wire click handlers
    const tabBar = document.getElementById('tab-bar');
    GAME_MODES.forEach(mode => {
      const btn = document.createElement('button');
      btn.className = 'tab';
      btn.dataset.tab = mode.id;
      btn.textContent = mode.label;
      btn.addEventListener('click', () => switchTab(mode.id));
      tabBar.appendChild(btn);
    });
    activateTab('play'); // default active tab (UI only — WASM initialized later in boot())

    // ── Arena game loop ───────────────────────────────────────────────────────
    async function runArena() {
      const myGen = ++arenaGeneration;
      arenaRunning = true;
      while (arenaRunning && arenaGeneration === myGen) {
        // Step all bots through the current hand
        while (true) {
          const result = JSON.parse(_arenaMod.step_bot());
          const state  = JSON.parse(_arenaMod.get_state());
          if (!result.done && result.seat != null && result.action_label) {
            const { verb, amount } = parseBotActionLabel(result.action_label);
            arenaAdapter?.pushEvent('action', result.seat, {
              verb, amount, pot_after: 0, to_call_after: 0,
            });
          }
          renderTableVisuals(state);   // not renderState — avoids triggering play-mode hand flow
          updateArenaStatus(state);
          if (result.done) { arenaAdapter?.poke(); break; }
          await new Promise(r => setTimeout(r, BOT_ACTION_MS));
          if (!arenaRunning || arenaGeneration !== myGen) return;
        }

        const state = JSON.parse(_arenaMod.get_state());
        if (state.phase === 'SessionOver') {
          const winner = [...state.players].sort((a, b) => b.chips - a.chips)[0];
          document.getElementById('arena-status').textContent =
            `Session over! Winner: ${winner.name} — ${state.hand_number} hands played.`;
          document.getElementById('arena-start-btn').textContent = 'New Arena';
          arenaRunning = false;
          return;
        }

        // Pause to show hand result, then auto-advance
        await new Promise(r => setTimeout(r, HAND_COMPLETE_MS));
        if (!arenaRunning || arenaGeneration !== myGen) return;
        noteHandCompleted('arena');
        // Advance the hand (syncs blind level, then calls next_hand()).
        const { state: nextState, blindLevel: newArenaLevel } = advanceHand(_arenaMod, state.hand_number, arenaBlindLevel);
        arenaBlindLevel = newArenaLevel;
        updateUrlState('arena', nextState.hand_number);
      }
    }

    function updateArenaStatus(state) {
      const active = state.players.filter(p => p.chips > 0);
      const leader = [...state.players].sort((a, b) => b.chips - a.chips)[0];
      document.getElementById('arena-status').textContent =
        `Hand #${state.hand_number} · Blinds ${state.small_blind}/${state.big_blind} · ${active.length} players left · Leader: ${leader.name} $${leader.chips.toLocaleString()}`;
    }

    document.getElementById('arena-start-btn').addEventListener('click', () => {
      ensureVoice();
      arenaRunning = false;  // stop any running loop before restarting
      arenaBlindLevel = 0;
      gameSeed = Date.now();
      resetHandsCompleted('arena');
      const state = JSON.parse(_arenaMod.init_bot_game(gameSeed));
      updateUrlState('arena', 1);
      renderTableVisuals(state);
      updateArenaStatus(state);
      document.getElementById('arena-status').textContent =
        `Hand #${state.hand_number} — Starting…`;
      document.getElementById('arena-start-btn').textContent = 'New Arena';
      activateArenaAudio();
      runArena();
    });

    // Enable the YAML download button once a hand has completed.
    function enableYamlDownload() {
      document.getElementById('btn-download-yaml').disabled = false;
    }

    // Enable the current-game debug button as soon as a game is initialized.
    function enableCurrentGameButton() {
      document.getElementById('btn-current-game').disabled = false;
    }
