// www/audio/adapters/live.js
//
// Live adapter: watches the Rust/WASM game state and emits GameEvents
// whenever something changes. The audio layer (voice.js) consumes these
// events; the same event shape is produced by the replay and all-bot
// adapters so everything downstream is source-agnostic.
//
// Field names are mapped to pkcore's get_state() JSON shape.
// See docs/audio-integration.md for architecture overview.

const _warned = new Set();
function warnOnce(key, msg) {
  if (_warned.has(key)) return;
  _warned.add(key);
  console.warn(`[live-adapter] ${msg}`);
}

function pick(obj, ...paths) {
  for (const path of paths) {
    let cur = obj;
    let ok = true;
    for (const key of path.split('.')) {
      if (cur == null || typeof cur !== 'object' || !(key in cur)) { ok = false; break; }
      cur = cur[key];
    }
    if (ok && cur !== undefined) return cur;
  }
  return undefined;
}

// Scan hero + players array for the seat where a boolean flag is true.
function findSeatWhere(raw, flag) {
  if (raw?.hero?.[flag]) return 0;
  for (const p of raw?.players ?? []) {
    if (p?.[flag]) return Number(p.seat);
  }
  return undefined;
}

// ===========================================================================
// EXTRACTOR — the shape-dependent layer, mapped to pkcore's get_state() JSON.
// ===========================================================================

const Extract = {
  handId(raw) {
    // pkcore uses hand_number
    const v = pick(raw, 'hand_number', 'hand_id', 'handId', 'current_hand.id');
    if (v === undefined) warnOnce('hand_id', 'no hand_number in get_state payload; using 0');
    return v ?? 0;
  },

  street(raw) {
    // pkcore returns e.g. 'Preflop', 'Flop', 'Turn', 'River'
    const v = pick(raw, 'street', 'phase', 'stage');
    return typeof v === 'string' ? v.toLowerCase() : 'idle';
  },

  pot(raw) {
    return Number(pick(raw, 'pot', 'total_pot', 'pot_total') ?? 0);
  },

  board(raw) {
    const v = pick(raw, 'board', 'community_cards', 'board_cards');
    return Array.isArray(v) ? v.map(String) : [];
  },

  heroHand(raw) {
    // pkcore: state.hero.hole_cards
    const v = pick(raw, 'hero.hole_cards', 'hero_cards', 'your_cards');
    return Array.isArray(v) ? v.map(String) : [];
  },

  buttonSeat(raw) {
    // pkcore: per-seat is_dealer flag rather than a top-level button_seat field
    return findSeatWhere(raw, 'is_dealer') ?? pick(raw, 'button_seat', 'button', 'dealer_seat');
  },

  sbSeat(raw) {
    return findSeatWhere(raw, 'is_sb') ?? pick(raw, 'sb_seat', 'small_blind_seat');
  },

  bbSeat(raw) {
    return findSeatWhere(raw, 'is_bb') ?? pick(raw, 'bb_seat', 'big_blind_seat');
  },

  toActSeat(raw) {
    // pkcore signals hero's turn via phase === 'WaitingForHuman'; no explicit to_act field
    if (raw?.phase === 'WaitingForHuman') return 0;
    return pick(raw, 'to_act', 'current_seat', 'action_on', 'turn_seat') ?? null;
  },

  toCall(raw) {
    return Number(pick(raw, 'to_call', 'call_amount', 'amount_to_call') ?? 0);
  },

  minRaise(raw) {
    return Number(pick(raw, 'min_raise', 'minimum_raise') ?? 0);
  },

  stacks(raw) {
    // pkcore: hero is seat 0 (separate field), bots are in players[]
    const out = {};
    if (raw?.hero?.chips != null) out[0] = Number(raw.hero.chips);
    for (const p of raw?.players ?? []) {
      if (p?.seat != null && p?.chips != null) out[Number(p.seat)] = Number(p.chips);
    }
    return out;
  },

  lastAction(raw) {
    // pkcore does not expose last_action/action_log in get_state().
    // Bot and hero actions are pushed via LiveAdapter.pushEvent() instead.
    const direct = pick(raw, 'last_action', 'lastAction');
    if (direct && typeof direct === 'object') {
      return {
        seat:     Number(direct.seat),
        verb:     String(direct.verb || direct.action || '').toLowerCase(),
        amount:   Number(direct.amount ?? 0),
        sequence: Number(direct.seq ?? direct.sequence ?? 0),
      };
    }
    const log = pick(raw, 'action_log', 'actions', 'hand.actions');
    if (Array.isArray(log) && log.length > 0) {
      const last = log[log.length - 1];
      return {
        seat:     Number(last.seat),
        verb:     String(last.verb || last.action || '').toLowerCase(),
        amount:   Number(last.amount ?? 0),
        sequence: log.length,
      };
    }
    return null;
  },

  showdown(raw) {
    // pkcore: hand result appears in last_result[] when phase === 'HandComplete'
    if (raw?.phase !== 'HandComplete') return null;
    const results = raw.last_result;
    if (!Array.isArray(results) || results.length === 0) return null;
    const winners = results.map(r => ({
      seat:           Number(r.seats?.[0] ?? 0),
      amount:         Number(r.amount ?? 0),
      hand_rank_text: String(r.hand ?? ''),
    }));
    return { shown: [], winners };
  },

  normalizeVerb(verb) {
    const map = {
      check: 'check', fold: 'fold', call: 'call', bet: 'bet',
      raise: 'raise', raise_to: 'raise', raises_to: 'raise',
      allin: 'allin', 'all-in': 'allin', all_in: 'allin',
      post_sb: 'post_sb', post_bb: 'post_bb',
      small_blind: 'post_sb', big_blind: 'post_bb',
    };
    return map[verb] ?? verb;
  },
};

// ===========================================================================
// NORMALIZED SNAPSHOT
// ===========================================================================

function snapshot(raw) {
  if (!raw || typeof raw !== 'object') return null;
  return {
    handId:     Extract.handId(raw),
    street:     Extract.street(raw),
    pot:        Extract.pot(raw),
    board:      Extract.board(raw),
    heroHand:   Extract.heroHand(raw),
    buttonSeat: Extract.buttonSeat(raw),
    sbSeat:     Extract.sbSeat(raw),
    bbSeat:     Extract.bbSeat(raw),
    toActSeat:  Extract.toActSeat(raw),
    toCall:     Extract.toCall(raw),
    minRaise:   Extract.minRaise(raw),
    stacks:     Extract.stacks(raw),
    lastAction: Extract.lastAction(raw),
    showdown:   Extract.showdown(raw),
  };
}

// ===========================================================================
// DIFF ENGINE — pure, source-agnostic.
// ===========================================================================

function diff(prev, next, ctx) {
  const events = [];
  const push = (kind, seat, data) => {
    events.push({
      id:      ++ctx.nextId,
      t_ms:    performance.now() - ctx.startedAtMs,
      hand_id: next.handId,
      kind, seat, data,
    });
  };

  // 1. New hand?
  if (!prev || prev.handId !== next.handId) {
    push('hand_start', null, {
      button_seat: next.buttonSeat,
      sb_seat:     next.sbSeat,
      bb_seat:     next.bbSeat,
      stacks:      { ...next.stacks },
    });
    if (next.heroHand.length > 0) {
      push('deal', 0, { to: 'hero', cards: next.heroHand.slice(), street: 'preflop' });
    }
  } else {
    if (prev.heroHand.length === 0 && next.heroHand.length > 0) {
      push('deal', 0, { to: 'hero', cards: next.heroHand.slice(), street: 'preflop' });
    }
  }

  // 2. Street change?
  const REAL_STREETS = new Set(['flop', 'turn', 'river']);
  const streetChanged =
    prev && prev.handId === next.handId &&
    prev.board.length !== next.board.length &&
    REAL_STREETS.has(next.street);
  if (streetChanged && next.board.length > 0) {
    push('street', null, { street: next.street, board: next.board.slice(), pot: next.pot });
  }

  // 3. New action? (via action_log sequence — not present in pkcore; use pushEvent instead)
  if (next.lastAction) {
    const sameHand = prev && prev.handId === next.handId;
    const prevSeq  = sameHand ? (prev.lastAction?.sequence ?? -1) : null;
    if (sameHand && next.lastAction.sequence > prevSeq) {
      const la = next.lastAction;
      push('action', la.seat, {
        verb:          Extract.normalizeVerb(la.verb),
        amount:        la.amount,
        pot_after:     next.pot,
        to_call_after: next.toCall,
      });
    }
  }

  // 4. Your turn?
  const wasYourTurn = prev?.toActSeat === 0;
  const isYourTurn  = next.toActSeat === 0;
  if (isYourTurn && !wasYourTurn) {
    push('your_turn', 0, {
      to_call:   next.toCall,
      pot:       next.pot,
      min_raise: next.minRaise,
      stack:     next.stacks[0] ?? 0,
    });
  }

  // 5. Showdown?
  if (next.showdown && !prev?.showdown) {
    push('showdown', null, {
      shown:   next.showdown.shown.slice(),
      winners: next.showdown.winners.slice(),
    });
  }

  // 6. Hand ended?
  const handEnded =
    prev && prev.handId === next.handId &&
    prev.street !== 'idle' && prev.street !== 'handcomplete' &&
    (next.street === 'idle' || next.street === 'handcomplete');
  if (handEnded) {
    const delta = {};
    for (const seat of Object.keys(next.stacks)) {
      const d = (next.stacks[seat] ?? 0) - (prev.stacks[seat] ?? 0);
      if (d !== 0) delta[seat] = d;
    }
    push('hand_end', null, { chip_delta: delta });
  }

  return events;
}

// ===========================================================================
// ADAPTER
// ===========================================================================

export class LiveAdapter {
  constructor({ getState, onEvent, intervalMs = 100, onSnapshot }) {
    if (typeof getState !== 'function') throw new Error('getState is required');
    if (typeof onEvent  !== 'function') throw new Error('onEvent is required');
    this.getState   = getState;
    this.onEvent    = onEvent;
    this.onSnapshot = onSnapshot;
    this.intervalMs = intervalMs;
    this._timer     = null;
    this._prev      = null;
    this._ctx       = { nextId: 0, startedAtMs: performance.now() };
  }

  start() {
    if (this._timer != null) return;
    this._ctx.startedAtMs = performance.now();
    this._tick();
    this._timer = setInterval(() => this._tick(), this.intervalMs);
  }

  stop() {
    if (this._timer != null) clearInterval(this._timer);
    this._timer = null;
  }

  poke() { this._tick(); }

  /**
   * Inject a pre-formed event directly to onEvent, bypassing the diff engine.
   * Use this for events that are only available from action return values
   * (e.g. step_bot() results), not from polling get_state().
   */
  pushEvent(kind, seat, data) {
    this.onEvent({
      id:      ++this._ctx.nextId,
      t_ms:    performance.now() - this._ctx.startedAtMs,
      hand_id: this._prev?.handId ?? 0,
      kind, seat, data,
    });
  }

  _tick() {
    let raw;
    try { raw = this.getState(); }
    catch (e) {
      warnOnce('getstate-throw', `getState() threw: ${e.message}`);
      return;
    }
    if (!raw) return;
    const snap = snapshot(raw);
    if (!snap) return;
    if (this.onSnapshot) this.onSnapshot(raw, snap);

    const events = diff(this._prev, snap, this._ctx);
    this._prev = snap;
    for (const ev of events) this.onEvent(ev);
  }
}

export { Extract, snapshot, diff };
