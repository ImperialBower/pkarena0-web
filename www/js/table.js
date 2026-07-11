// design2.0 table component. Builds felt + 9 seats + center inside rootEl and
// returns an update(view) API. Never clears rootEl — it only appends.
import { makeCard } from './cards.js';

// Physical seat coordinates (percent), desktop (x,y) and mobile (mx,my),
// from the SEATS array in docs/design2.0/Poker Table.dc.html. Seat 0 = hero.
const SEAT_POS = [
  { x: 71, y: 90, mx: 50, my: 92 },
  { x: 29, y: 90, mx: 13, my: 82 },
  { x: 8,  y: 64, mx: 8,  my: 60 },
  { x: 8,  y: 30, mx: 8,  my: 36 },
  { x: 27, y: 7,  mx: 16, my: 13 },
  { x: 50, y: 4,  mx: 50, my: 6  },
  { x: 73, y: 7,  mx: 84, my: 13 },
  { x: 92, y: 30, mx: 92, my: 36 },
  { x: 92, y: 64, mx: 92, my: 60 },
];
const POS_NAMES = ['BTN', 'SB', 'BB', 'UTG', 'UTG+1', 'MP', 'LJ', 'HJ', 'CO'];
const SLOT_LABELS = ['FLOP', 'FLOP', 'FLOP', 'TURN', 'RIVER'];

export function positionTag(seat, dealerSeat) {
  if (dealerSeat == null) return '';
  return POS_NAMES[((seat - dealerSeat) % 9 + 9) % 9];
}

export function pillVariant(label) {
  const l = (label ?? '').toLowerCase();
  if (!l) return null;
  if (l.includes('fold')) return 'fold';
  if (l.includes('raise') || l.includes('bet') || l.includes('all-in')) return 'raise';
  if (l.includes('post') || l.includes('sb ') || l.includes('bb ') || l.includes('blind')) return 'blind';
  if (l.includes('win') || l.includes('to act')) return 'hero';
  return 'blind'; // neutral: check / call
}

const fmt = n => '$' + (n ?? 0).toLocaleString('en-US');

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function buildSeat(i) {
  const seat = el('div', 'seat');
  seat.dataset.seat = String(i);
  seat.style.setProperty('--x', SEAT_POS[i].x + '%');
  seat.style.setProperty('--y', SEAT_POS[i].y + '%');
  seat.style.setProperty('--mx', SEAT_POS[i].mx + '%');
  seat.style.setProperty('--my', SEAT_POS[i].my + '%');

  const stack = el('div', 'seat-stack-col');
  const cards = el('div', 'seat-cards');
  const ring = el('div', 'seat-ring');
  const plate = el('div', 'seat-plate');
  const row1 = el('div', 'seat-row1');
  const name = el('a', 'seat-name', '—');
  name.rel = 'noopener noreferrer';
  name.target = '_blank';
  const tag = el('span', 'seat-tag');
  row1.append(name, tag);
  const row2 = el('div', 'seat-row2');
  row2.append(el('span', 'seat-stack'), el('span', 'seat-bb'));
  const hud = el('div', 'seat-hud');
  plate.append(row1, row2, hud);
  const badge = el('div', 'seat-badge');
  ring.append(plate, badge);
  const pill = el('div', 'seat-pill');
  pill.append(el('span', 'act-full'), el('span', 'act-short'));
  const chipamt = el('div', 'seat-chipamt');
  stack.append(cards, ring, pill, chipamt);
  seat.append(stack);
  return seat;
}

// Abbreviate an action label for the mobile pill. Handles both button-style
// labels ("Raise $400") and pkcore's verb labels ("raises to $400", "calls $100").
function shortLabel(label) {
  return label
    .replace(/^raises?\s*(?:to\s*)?/i, 'R ').replace(/^bets?\s*/i, 'B ')
    .replace(/^calls?\s*/i, 'C ').replace(/^checks?.*/i, 'CHK')
    .replace(/^folds?.*/i, 'FOLD').replace(/^all-?in\s*/i, 'AI ')
    .toUpperCase().trim();
}

export function createTable(rootEl, { replay = false } = {}) {
  const zone = el('div', 'table-inner');
  const felt = el('div', 'felt');
  const ringOuter = el('div', 'felt-ring');
  ringOuter.append(el('div', 'felt-ring-inner'));
  const center = el('div', 'table-center');
  const pot = el('div', 'pot', 'POT $0');
  const board = el('div', 'board');
  const slots = SLOT_LABELS.map(lbl => {
    const s = el('div', 'board-slot');
    s.append(el('span', 'slot-label', lbl));
    board.append(s);
    return s;
  });
  const label = el('div', 'table-label', "NO LIMIT HOLD'EM");
  center.append(pot, board, label);
  zone.append(felt, ringOuter, center);
  const seats = [];
  for (let i = 0; i < 9; i++) {
    const s = buildSeat(i);
    seats.push(s);
    zone.append(s);
  }
  rootEl.append(zone);

  // Mobile seat-list + board-strip. Hidden on desktop and in table-mode via CSS;
  // populated from the same view on every update().
  const listWrap = el('div', 'seat-list-wrap');
  const strip = el('div', 'board-strip');
  const stripPot = el('span', 'strip-pot', 'POT $0');
  const stripBoard = el('div', 'strip-board');
  const stripSlots = SLOT_LABELS.map(() => {
    const s = el('div', 'strip-slot');
    stripBoard.append(s);
    return s;
  });
  const stripBlinds = el('span', 'strip-blinds', '');
  strip.append(stripPot, stripBoard, stripBlinds);
  const list = el('div', 'seat-list');
  const listRows = [];
  for (let i = 0; i < 9; i++) {
    const row = el('div', 'list-row');
    row.dataset.listSeat = String(i);
    const tag = el('span', 'list-tag');
    const mid = el('div', 'list-mid');
    mid.append(el('span', 'list-name', '—'), el('span', 'list-hud'));
    const right = el('div', 'list-right');
    right.append(el('span', 'list-stack'), el('span', 'list-bb'));
    const pill = el('span', 'list-pill');
    row.append(tag, mid, right, pill);
    list.append(row);
    listRows.push(row);
  }
  listWrap.append(strip, list);
  rootEl.append(listWrap);

  const q = (i, sel) => seats[i].querySelector(sel);
  const actionLabels = new Array(9).fill(null);

  function setActionLabel(i, lbl) {
    actionLabels[i] = lbl;
    const pill = q(i, '.seat-pill');
    const variant = pillVariant(lbl);
    pill.className = 'seat-pill' + (variant ? ' pill-' + variant : '');
    q(i, '.act-full').textContent = lbl ?? '';
    q(i, '.act-short').textContent = lbl ? shortLabel(lbl) : '';
    pill.style.visibility = lbl ? 'visible' : 'hidden';
  }

  function renderSeat(i, p, view) {
    const seatEl = seats[i];
    if (!p || p.state === 'Out' || !p.name) {
      seatEl.className = 'seat seat-empty';
      q(i, '.seat-name').textContent = '—';
      q(i, '.seat-cards').replaceChildren();
      q(i, '.seat-badge').style.display = 'none';
      setActionLabel(i, null);
      q(i, '.seat-chipamt').style.visibility = 'hidden';
      return;
    }
    seatEl.className = 'seat' + (p.state === 'Fold' ? ' seat-fold' : '');

    const nameEl = q(i, '.seat-name');
    const emoji = view.emoji && i !== view.heroSeat ? view.emoji(p.name) : '';
    nameEl.textContent = (emoji ? emoji + ' ' : '') + p.name;
    const href = view.nameHref && i !== view.heroSeat ? view.nameHref(p.name) : null;
    if (href) nameEl.setAttribute('href', href);
    else nameEl.removeAttribute('href');

    q(i, '.seat-tag').textContent = positionTag(i, view.dealerSeat);

    const allIn = p.state === 'AllIn'
      ? (view.allInAmounts?.get(i) ?? (p.bet > 0 ? p.bet : null))
      : null;
    const stackEl = q(i, '.seat-stack');
    stackEl.textContent = allIn != null ? fmt(allIn) : fmt(p.chips);
    stackEl.classList.toggle('allin', allIn != null);
    q(i, '.seat-bb').textContent =
      view.bigBlind > 0 && allIn == null
        ? Math.round((p.chips ?? 0) / view.bigBlind) + 'BB' : '';

    q(i, '.seat-hud').style.display = view.showHud ? '' : 'none';

    const chipEl = q(i, '.seat-chipamt');
    if (p.bet > 0 && p.state !== 'AllIn') {
      chipEl.textContent = fmt(p.bet);
      chipEl.style.visibility = 'visible';
    } else {
      chipEl.style.visibility = 'hidden';
    }

    const badge = q(i, '.seat-badge');
    const b = p.is_dealer ? 'D' : p.is_sb ? 'SB' : p.is_bb ? 'BB' : null;
    badge.textContent = b ?? '';
    badge.style.display = b ? '' : 'none';

    const ring = q(i, '.seat-ring');
    ring.classList.toggle('hero-ring', i === view.heroSeat);
    ring.classList.toggle('to-act',
      view.currentSeat === i || (view.heroTurn && i === view.heroSeat));

    // Seat hole cards (face-down markers / showdown reveal). Hero's big cards
    // live in the dock, so the (non-replay) hero seat shows none.
    const cardsEl = q(i, '.seat-cards');
    cardsEl.replaceChildren();
    if (replay || i !== view.heroSeat) {
      for (const c of p.hole_cards ?? []) {
        const node = makeCard(c, 'mini');
        if (node) cardsEl.append(node);
      }
    }

    if (actionLabels[i] == null && (p.state === 'Fold' || p.state === 'AllIn')) {
      setActionLabel(i, p.state === 'Fold' ? 'Fold' : 'All-In');
    }
  }

  // Mobile seat-list row — same data as renderSeat, laid out as a flat row.
  function renderListRow(i, p, view) {
    const row = listRows[i];
    const q2 = sel => row.querySelector(sel);
    if (!p || p.state === 'Out' || !p.name) {
      row.className = 'list-row list-empty';
      q2('.list-tag').textContent = '';
      q2('.list-name').textContent = '—';
      q2('.list-hud').textContent = '';
      q2('.list-stack').textContent = '';
      q2('.list-bb').textContent = '';
      q2('.list-pill').className = 'list-pill';
      q2('.list-pill').textContent = '';
      return;
    }
    row.className = 'list-row'
      + (i === view.heroSeat ? ' list-hero' : '')
      + (p.state === 'Fold' ? ' list-fold' : '');
    q2('.list-tag').textContent = positionTag(i, view.dealerSeat);
    const emoji = view.emoji && i !== view.heroSeat ? view.emoji(p.name) : '';
    q2('.list-name').textContent = (emoji ? emoji + ' ' : '') + p.name;
    q2('.list-hud').style.display = view.showHud ? '' : 'none';
    const allIn = p.state === 'AllIn'
      ? (view.allInAmounts?.get(i) ?? (p.bet > 0 ? p.bet : null)) : null;
    q2('.list-stack').textContent = allIn != null ? fmt(allIn) : fmt(p.chips);
    q2('.list-bb').textContent =
      view.bigBlind > 0 && allIn == null
        ? Math.round((p.chips ?? 0) / view.bigBlind) + 'BB' : '';
    const lbl = actionLabels[i];
    const variant = pillVariant(lbl);
    const pill = q2('.list-pill');
    pill.className = 'list-pill' + (variant ? ' pill-' + variant : '');
    pill.textContent = lbl ? shortLabel(lbl) : '';
    pill.style.visibility = lbl ? 'visible' : 'hidden';
  }

  function update(view) {
    pot.textContent = 'POT ' + fmt(view.pot);
    label.textContent = view.smallBlind
      ? `NO LIMIT HOLD'EM · ${view.smallBlind}/${view.bigBlind}` : "NO LIMIT HOLD'EM";
    slots.forEach((slot, idx) => {
      const c = view.board?.[idx] ?? null;
      slot.replaceChildren();
      if (c) slot.append(makeCard(c, 'board'));
      else slot.append(el('span', 'slot-label', SLOT_LABELS[idx]));
    });
    const bySeat = new Map((view.seats ?? []).map(p => [p.seat, p]));
    for (let i = 0; i < 9; i++) renderSeat(i, bySeat.get(i) ?? null, view);

    stripPot.textContent = 'POT ' + fmt(view.pot);
    stripBlinds.textContent = view.smallBlind ? `${view.smallBlind}/${view.bigBlind}` : '';
    stripSlots.forEach((slot, idx) => {
      const c = view.board?.[idx] ?? null;
      slot.replaceChildren();
      if (c) slot.append(makeCard(c, 'mini'));
    });
    for (let i = 0; i < 9; i++) renderListRow(i, bySeat.get(i) ?? null, view);
  }

  function clear() {
    for (let i = 0; i < 9; i++) {
      renderSeat(i, null, { heroSeat: null });
      actionLabels[i] = null;
    }
    pot.textContent = 'POT $0';
    slots.forEach((slot, idx) => {
      slot.replaceChildren(el('span', 'slot-label', SLOT_LABELS[idx]));
    });
    stripPot.textContent = 'POT $0';
    stripBlinds.textContent = '';
    stripSlots.forEach(slot => slot.replaceChildren());
    for (let i = 0; i < 9; i++) renderListRow(i, null, { heroSeat: null });
  }

  return { update, setActionLabel, clear };
}
