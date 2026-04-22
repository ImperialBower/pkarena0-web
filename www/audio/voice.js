// www/audio/voice.js
//
// Voice clip stitcher for pkarena0-web.
//
// Loads pre-recorded WAV atoms (see pkarena0-voice-script.md) and concatenates
// them into full spoken lines at runtime. Playback is sample-accurate via
// Web Audio's scheduling — not chained <audio> elements, which have gap issues.
//
// Usage:
//   import { Voice } from './voice.js';
//   const voice = new Voice({ basePath: './audio/voice/' });
//   await voice.preload();                 // loads all atoms into AudioBuffers
//   voice.say.seatAction(3, 'raises_to', 450);
//   voice.say.yourHand(['As', 'Kh']);
//   voice.say.flop(['7c', '2d', 'Ts']);
//   voice.cancel();                        // interrupt anything in progress
//
// No dependencies. Pure Web Audio API + fetch.

// ---------------------------------------------------------------------------
// Atom inventory — MUST match filenames in the voice-actor script.
// If a clip is missing the player falls back to SpeechSynthesis for that line.
// ---------------------------------------------------------------------------

const ATOMS = {
  // Numbers (Section 1)
  'num_zero': 'zero', 'num_one': 'one', 'num_two': 'two', 'num_three': 'three',
  'num_four': 'four', 'num_five': 'five', 'num_six': 'six', 'num_seven': 'seven',
  'num_eight': 'eight', 'num_nine': 'nine', 'num_ten': 'ten',
  'num_eleven': 'eleven', 'num_twelve': 'twelve', 'num_thirteen': 'thirteen',
  'num_fourteen': 'fourteen', 'num_fifteen': 'fifteen', 'num_sixteen': 'sixteen',
  'num_seventeen': 'seventeen', 'num_eighteen': 'eighteen', 'num_nineteen': 'nineteen',
  'num_twenty': 'twenty', 'num_thirty': 'thirty', 'num_forty': 'forty',
  'num_fifty': 'fifty', 'num_sixty': 'sixty', 'num_seventy': 'seventy',
  'num_eighty': 'eighty', 'num_ninety': 'ninety',
  'num_hundred': 'hundred', 'num_thousand': 'thousand', 'num_and': 'and',

  // Card ranks (Section 2a)
  'rank_two': 'two', 'rank_three': 'three', 'rank_four': 'four', 'rank_five': 'five',
  'rank_six': 'six', 'rank_seven': 'seven', 'rank_eight': 'eight', 'rank_nine': 'nine',
  'rank_ten': 'ten', 'rank_jack': 'jack', 'rank_queen': 'queen', 'rank_king': 'king',
  'rank_ace': 'ace',

  // Suits (Section 2b)
  'suit_of_clubs': 'of clubs', 'suit_of_diamonds': 'of diamonds',
  'suit_of_hearts': 'of hearts', 'suit_of_spades': 'of spades',

  // Rank plurals (Section 2c)
  'ranks_twos': 'twos', 'ranks_threes': 'threes', 'ranks_fours': 'fours',
  'ranks_fives': 'fives', 'ranks_sixes': 'sixes', 'ranks_sevens': 'sevens',
  'ranks_eights': 'eights', 'ranks_nines': 'nines', 'ranks_tens': 'tens',
  'ranks_jacks': 'jacks', 'ranks_queens': 'queens', 'ranks_kings': 'kings',
  'ranks_aces': 'aces',

  // Suit plurals (Section 2d)
  'suits_clubs': 'clubs', 'suits_diamonds': 'diamonds',
  'suits_hearts': 'hearts', 'suits_spades': 'spades',

  // Seats (Section 3a)
  'seat_one': 'Seat one', 'seat_two': 'Seat two', 'seat_three': 'Seat three',
  'seat_four': 'Seat four', 'seat_five': 'Seat five', 'seat_six': 'Seat six',
  'seat_seven': 'Seat seven', 'seat_eight': 'Seat eight', 'seat_you': 'You',

  // Positions (Section 3b)
  'pos_button': 'on the button', 'pos_small_blind': 'small blind',
  'pos_big_blind': 'big blind', 'pos_under_the_gun': 'under the gun',
  'pos_cutoff': 'cutoff', 'pos_hijack': 'hijack',

  // Actions (Section 4)
  'act_checks': 'checks', 'act_folds': 'folds', 'act_mucks': 'mucks',
  'act_calls': 'calls', 'act_bets': 'bets', 'act_raises_to': 'raises to',
  'act_all_in_for': 'is all in for',
  'act_posts_small_blind': 'posts the small blind',
  'act_posts_big_blind': 'posts the big blind',
  'act_sits_out': 'sits out',

  // Streets and deals (Section 5)
  'street_preflop': 'Preflop.', 'street_flop': 'The flop:',
  'street_turn': 'The turn:', 'street_river': 'The river:',
  'deal_your_hand': 'Your hand:', 'deal_new_hand': 'New hand.',

  // Your turn (Section 6)
  'turn_action_on_you': 'Action on you.', 'turn_to_call': 'To call:',
  'turn_pot': 'Pot:', 'turn_your_stack': 'Your stack:',
  'turn_check_or_bet': 'Check or bet.', 'turn_min_raise': 'Minimum raise:',

  // Showdown (Section 7)
  'sd_showdown': 'Showdown.', 'sd_shows': 'shows', 'sd_with': 'with',
  'sd_wins': 'wins', 'sd_ties': 'Split pot.',
  'sd_uncalled_returned': 'Uncalled bet returned.',
  'hand_high_card': 'high card', 'hand_pair_of': 'a pair of',
  'hand_two_pair': 'two pair,', 'hand_three_of_a_kind': 'three of a kind,',
  'hand_straight': 'a straight,', 'hand_flush': 'a flush,',
  'hand_full_house': 'a full house,', 'hand_four_of_a_kind': 'four of a kind,',
  'hand_straight_flush': 'a straight flush,', 'hand_royal_flush': 'a royal flush.',
  'hand_kicker': 'kicker', 'hand_over': 'over', 'hand_high': 'high',
  'result_you_win': 'You win.', 'result_you_lose': 'You lose.',
  'result_you_fold': 'You fold.', 'result_chop': 'Chop.',

  // Queries (Section 8)
  'query_your_hand_is': 'Your hand is:', 'query_the_board_is': 'The board is:',
  'query_board_empty': 'The board is empty.', 'query_pot_is': 'Pot is:',
  'query_to_call': 'To call:', 'query_nothing_to_call': 'Nothing to call.',
  'query_action_on': 'Action on', 'query_your_stack_is': 'Your stack is:',
  'query_button_is_seat': 'Button is seat', 'query_button_is_you': 'Button is you.',
  'query_players_remaining': 'Players remaining:',

  // Session / meta (Section 9)
  'sess_new_game': 'New game.', 'sess_blinds_are': 'Blinds are:',
  'sess_game_over': 'Game over.', 'sess_you_busted': 'You are out of chips.',
  'sess_you_are_the_winner': 'You are the winner.',
  'sess_paused': 'Paused.', 'sess_resumed': 'Resumed.',
  'sess_replay_starting': 'Replay starting.', 'sess_replay_ended': 'Replay ended.',
  'sess_bot_session_starting': 'Bot session starting.',
  'sess_bot_session_complete': 'Bot session complete.',
  'err_illegal_action': 'Illegal action.', 'err_not_your_turn': 'Not your turn.',
  'err_insufficient_chips': 'Insufficient chips.',

  // Currency / glue (Section 10)
  'money_dollars': 'dollars', 'money_chips': 'chips',
  'glue_of': 'of', 'glue_and': 'and', 'glue_wins': 'wins', 'glue_with': 'with',
};

const SUIT_CODE = { c: 'clubs', d: 'diamonds', h: 'hearts', s: 'spades' };
const RANK_CODE = {
  '2': 'two', '3': 'three', '4': 'four', '5': 'five', '6': 'six',
  '7': 'seven', '8': 'eight', '9': 'nine', T: 'ten', J: 'jack',
  Q: 'queen', K: 'king', A: 'ace',
};

// ---------------------------------------------------------------------------
// Number → atom list. Handles 0..999,999. Style: "four hundred and fifty."
// Poker amounts rarely exceed a few hundred thousand; extend if you go higher.
// ---------------------------------------------------------------------------

function numberToAtoms(n) {
  n = Math.floor(Math.abs(Number(n) || 0));
  if (n === 0) return ['num_zero'];
  const out = [];
  const thousands = Math.floor(n / 1000);
  const rest = n % 1000;
  if (thousands > 0) {
    out.push(...under1000(thousands), 'num_thousand');
    if (rest > 0 && rest < 100) out.push('num_and');
  }
  if (rest > 0) out.push(...under1000(rest));
  return out;
}

function under1000(n) {
  const out = [];
  const hundreds = Math.floor(n / 100);
  const rest = n % 100;
  if (hundreds > 0) {
    out.push(`num_${ones(hundreds)}`, 'num_hundred');
    if (rest > 0) out.push('num_and');
  }
  if (rest > 0) out.push(...under100(rest));
  return out;
}

function under100(n) {
  if (n < 20) return [`num_${ones(n)}`];
  const tens = Math.floor(n / 10);
  const units = n % 10;
  const tensKey = ['', '', 'twenty', 'thirty', 'forty', 'fifty',
                   'sixty', 'seventy', 'eighty', 'ninety'][tens];
  return units === 0 ? [`num_${tensKey}`] : [`num_${tensKey}`, `num_${ones(units)}`];
}

function ones(n) {
  return ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven',
          'eight', 'nine', 'ten', 'eleven', 'twelve', 'thirteen', 'fourteen',
          'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen'][n];
}

// ---------------------------------------------------------------------------
// Card code → atom list. Accepts "As", "Td", "9h", etc.
// ---------------------------------------------------------------------------

function cardToAtoms(code) {
  const rank = RANK_CODE[code[0]];
  const suit = SUIT_CODE[code[1]?.toLowerCase()];
  if (!rank || !suit) {
    console.warn(`[voice] unknown card code: ${code}`);
    return [];
  }
  return [`rank_${rank}`, `suit_of_${suit}`];
}

function seatAtom(seat) {
  if (seat === 0 || seat === 'hero') return 'seat_you';
  const names = [null, 'one', 'two', 'three', 'four',
                 'five', 'six', 'seven', 'eight'];
  return names[seat] ? `seat_${names[seat]}` : null;
}

// ---------------------------------------------------------------------------
// The Voice class — loads atoms, schedules playback, exposes `say.*` helpers.
// ---------------------------------------------------------------------------

export class Voice {
  /**
   * @param {object} opts
   * @param {string} opts.basePath    Directory containing atom .wav files.
   * @param {AudioContext} [opts.ctx] Existing AudioContext, or one is created.
   * @param {number} [opts.gap]       Seconds of silence between atoms. Default 0.03.
   * @param {boolean} [opts.ttsFallback] If true, missing atoms fall back to
   *                                     SpeechSynthesis. Default true.
   */
  constructor({ basePath, ctx, gap = 0.03, ttsFallback = true } = {}) {
    this.basePath = basePath.endsWith('/') ? basePath : basePath + '/';
    this.ctx = ctx || new (window.AudioContext || window.webkitAudioContext)();
    this.gap = gap;
    this.ttsFallback = ttsFallback;
    this.buffers = new Map();         // atom name -> AudioBuffer
    this.missing = new Set();         // atoms that failed to load
    this.master = this.ctx.createGain();
    this.pan = this.ctx.createStereoPanner();
    this.master.connect(this.pan).connect(this.ctx.destination);
    this._activeSources = [];         // for cancel()
    this._nextStartAt = 0;            // scheduling cursor
    this._ttsTimer = null;            // debounce handle for TTS fallback

    // Convenience façade
    this.say = this._buildSayAPI();
  }

  /**
   * Load all atoms in parallel. Failed loads are tolerated — the stitcher
   * will skip them and (optionally) fall back to SpeechSynthesis.
   * Returns { loaded, missing } counts.
   */
  async preload() {
    const names = Object.keys(ATOMS);
    await Promise.all(names.map(name => this._loadAtom(name)));
    return {
      loaded: this.buffers.size,
      missing: this.missing.size,
      missingList: [...this.missing],
    };
  }

  async _loadAtom(name) {
    try {
      const res = await fetch(`${this.basePath}${name}.wav`);
      if (!res.ok) throw new Error(`${res.status}`);
      const arr = await res.arrayBuffer();
      const buf = await this.ctx.decodeAudioData(arr);
      this.buffers.set(name, buf);
    } catch (e) {
      this.missing.add(name);
    }
  }

  setVolume(v) { this.master.gain.value = Math.max(0, Math.min(1, v)); }

  /** Pan value -1 (left) to +1 (right). Use per-seat panning. */
  setPan(p) { this.pan.pan.value = Math.max(-1, Math.min(1, p)); }

  /** Pan helper that maps a seat index to a stereo position. */
  panForSeat(seat) {
    // Seats 1-4 on the left, 5-8 on the right, you in the center.
    if (seat === 0) return 0;
    const spread = { 1: -1.0, 2: -0.7, 3: -0.4, 4: -0.15,
                     5:  0.15, 6:  0.4, 7:  0.7, 8:  1.0 };
    return spread[seat] ?? 0;
  }

  /** Cancel anything currently speaking or queued. */
  cancel() {
    for (const src of this._activeSources) {
      try { src.stop(); } catch {}
    }
    this._activeSources = [];
    this._nextStartAt = this.ctx.currentTime;
    // Clear any pending deferred TTS. We intentionally do NOT call
    // speechSynthesis.cancel() here: Chrome has a bug where cancel() followed
    // closely by speak() silently drops the new utterance. Instead we let any
    // in-flight TTS finish naturally; the debounce timer in speak() ensures
    // only the latest utterance in a burst actually fires.
    clearTimeout(this._ttsTimer);
    this._ttsTimer = null;
  }

  /**
   * Schedule a list of atom names to play back-to-back with small gaps.
   * @param {string[]} atoms    atom keys in order
   * @param {object}   opts
   * @param {number}   opts.seat  if given, pans via panForSeat (one-off)
   * @param {number}   opts.rate  playback rate (1.0 = normal, >1 = faster)
   * @returns {Promise<void>} resolves when the utterance ends
   */
  speak(atoms, { seat, rate = 1.0 } = {}) {
    if (this.ctx.state === 'suspended') this.ctx.resume();

    // Interrupting: any new speak() starts fresh
    this.cancel();

    const startPan = this.pan.pan.value;
    if (typeof seat === 'number') this.setPan(this.panForSeat(seat));

    const now = this.ctx.currentTime;
    this._nextStartAt = now;

    const missingForTTS = [];
    for (const name of atoms) {
      const buf = this.buffers.get(name);
      if (!buf) {
        missingForTTS.push(name);
        continue;
      }
      const src = this.ctx.createBufferSource();
      src.buffer = buf;
      src.playbackRate.value = rate;
      src.connect(this.master);
      src.start(this._nextStartAt);
      this._activeSources.push(src);
      this._nextStartAt += buf.duration / rate + this.gap;
    }

    // If any atoms are missing, speak the entire line via TTS instead —
    // mixing recorded + synthesized within one sentence sounds bad.
    // Defer by 50 ms so that rapid back-to-back speak() calls (e.g. hand_start
    // + deal + your_turn all in one diff tick) only produce one utterance — the
    // last one — and the cancel()+immediate-speak Chrome bug is avoided.
    if (missingForTTS.length && this.ttsFallback) {
      const text = atoms.map(a => ATOMS[a] || '').filter(Boolean).join(' ');
      this._ttsTimer = setTimeout(() => this._speakTTS(text), 50);
    }

    const endsAt = this._nextStartAt;
    if (typeof seat === 'number') {
      // Restore pan after the utterance finishes
      setTimeout(() => this.setPan(startPan),
                 Math.max(0, (endsAt - this.ctx.currentTime) * 1000));
    }

    return new Promise(resolve => {
      const ms = Math.max(0, (endsAt - this.ctx.currentTime) * 1000);
      setTimeout(resolve, ms);
    });
  }

  _speakTTS(text) {
    if (!('speechSynthesis' in window)) return;
    const u = new SpeechSynthesisUtterance(text);
    u.rate = 1.05;
    speechSynthesis.speak(u);
  }

  // -------------------------------------------------------------------------
  // High-level helpers. These are the functions your event handlers call.
  // -------------------------------------------------------------------------

  _buildSayAPI() {
    const v = this;
    return {
      // "Seat three raises to four hundred and fifty."
      seatAction(seat, verb, amount) {
        const atoms = [seatAtom(seat)];
        const verbMap = {
          check: ['act_checks'],
          fold: ['act_folds'],
          muck: ['act_mucks'],
          call: ['act_calls', ...numberToAtoms(amount)],
          bet: ['act_bets', ...numberToAtoms(amount)],
          raise: ['act_raises_to', ...numberToAtoms(amount)],
          raises_to: ['act_raises_to', ...numberToAtoms(amount)],
          allin: ['act_all_in_for', ...numberToAtoms(amount)],
          post_sb: ['act_posts_small_blind'],
          post_bb: ['act_posts_big_blind'],
        };
        atoms.push(...(verbMap[verb] || []));
        return v.speak(atoms.filter(Boolean), { seat });
      },

      // "Your hand: ace of spades, king of hearts."
      yourHand(cards) {
        const atoms = ['deal_your_hand'];
        for (const c of cards) atoms.push(...cardToAtoms(c));
        return v.speak(atoms, { seat: 0 });
      },

      flop(cards)  { return v.speak(['street_flop',  ...cards.flatMap(cardToAtoms)]); },
      turn(card)   { return v.speak(['street_turn',  ...cardToAtoms(card)]); },
      river(card)  { return v.speak(['street_river', ...cardToAtoms(card)]); },

      // "Action on you. To call: two hundred. Pot: six hundred and fifty."
      yourTurn({ toCall, pot, stack }) {
        const atoms = ['turn_action_on_you'];
        if (toCall > 0) atoms.push('turn_to_call', ...numberToAtoms(toCall));
        else atoms.push('turn_check_or_bet');
        if (pot != null)   atoms.push('turn_pot', ...numberToAtoms(pot));
        if (stack != null) atoms.push('turn_your_stack', ...numberToAtoms(stack));
        return v.speak(atoms, { seat: 0 });
      },

      // "Showdown. Seat five wins with two pair, aces and kings."
      showdownWin(seat, handRankAtoms) {
        return v.speak(
          ['sd_showdown', seatAtom(seat), 'sd_wins', 'sd_with', ...handRankAtoms],
          { seat }
        );
      },

      // Simple pass-throughs for meta lines.
      line(...atoms) { return v.speak(atoms); },

      // Direct number / card helpers (for hotkey queries).
      number(n) { return v.speak(numberToAtoms(n)); },
      card(c)   { return v.speak(cardToAtoms(c)); },
    };
  }
}

// ---------------------------------------------------------------------------
// Exports for testing and for event-adapter code to reuse.
// ---------------------------------------------------------------------------

export { ATOMS, numberToAtoms, cardToAtoms, seatAtom };
