// <pk-table> — pkarena0-web in someone else's page (EPIC-56).
//
// Two lines mount it:
//
//   <script type="module" src="pk-table.js"></script>
//   <pk-table seed="42"><img src="table.png" alt="…"></pk-table>
//
// The <img> is the whole of what paper, script-off and file:// readers see.
// The element adds a Deal button under it and fetches nothing until Deal is
// pressed; then it imports the engine (its own instance), swaps the picture
// for the live table, and plays one hand at a time. If the engine will not
// load, the picture stays and the status line says so. The rules this file
// keeps, numbered 1–8, are tested in tests/embed.spec.ts.
import { createTable } from '../table.js';
import { makeCard }    from '../cards.js';
import { buttonsFor }  from '../actions.js';

const HERE = new URL('./', import.meta.url);
let nextInstance = 0;
// pk-table.css is fetched once per page and shared by every instance.
const sheets = new Map();

const POS_TAGS = ['BTN', 'SB', 'BB', 'UTG', 'UTG+1', 'MP', 'LJ', 'HJ', 'CO'];
const ACT_OF = { Fold: 'fold', Check: 'check', Call: 'call', allin: 'allin', undo: 'undo', 'bet-open': 'raise' };

// The pre-Deal look: enough for a button and a line of text. The full sheet
// arrives with the engine (rule 1: nothing fetched before Deal).
const BOOT_CSS = `
  :host { display: block; }
  .pk-frame { font: 14px/1.4 system-ui, sans-serif; }
  .pk-actions { padding: 6px 0; }
  .pk-actions button { font: inherit; font-weight: 600; padding: 8px 16px; border-radius: 8px; border: 1px solid currentColor; background: transparent; color: inherit; cursor: pointer; }
  .pk-actions button:disabled { opacity: .45; cursor: default; }
  .pk-status { font: 12px/1.4 ui-monospace, Menlo, Consolas, monospace; min-height: 1.4em; opacity: .8; }
  .pk-badge { font: 10px ui-monospace, Menlo, Consolas, monospace; opacity: .55; text-align: right; }
  .pk-zone[hidden], .pk-raise[hidden] { display: none; }
  .pk-frame.pk-live slot { display: none; }
`;

export class PkTable extends HTMLElement {
  #root; #zone; #hero; #status; #actions; #raise; #badge; #dealBtn;
  #mod = null; #table = null; #state = null; #hands = 0; #busy = false;
  #id = nextInstance++;

  connectedCallback() {
    if (this.#root) return;
    this.#root = this.attachShadow({ mode: 'open' });
    const boot = new CSSStyleSheet();
    boot.replaceSync(BOOT_CSS);
    this.#root.adoptedStyleSheets = [boot];

    const frame = el('div', 'pk-frame');
    const stage = el('div', 'pk-stage');
    stage.append(el('slot'));                       // the host's <img>, untouched
    this.#zone = el('div', 'pk-zone'); this.#zone.hidden = true;
    stage.append(this.#zone);
    this.#hero = el('div', 'pk-hero');
    this.#status = el('div', 'pk-status');
    this.#status.setAttribute('aria-live', 'polite');
    this.#actions = el('div', 'pk-actions');
    this.#raise = el('div', 'pk-raise'); this.#raise.hidden = true;
    this.#badge = el('div', 'pk-badge', 'pkarena0-web');
    frame.append(stage, this.#hero, this.#status, this.#actions, this.#raise, this.#badge);
    this.#root.append(frame);

    if (this.hasAttribute('bots')) {
      console.warn('<pk-table bots> is reserved (pkarena0-web EPIC-54) and ignored: eight bots.');
    }
    this.#showDeal();
    this.dataset.phase = 'idle';
  }

  // ── Public ────────────────────────────────────────────────────────────────

  /** The engine's state as `get_state()` last returned it, or null. */
  getState() { return this.#state; }

  /** Re-render the table and the action row from the last state (rule 6). */
  refresh() { if (this.#state) this.#render(this.#state, { row: true }); }

  /** Press Deal: load the engine on first use (rule 2), start a session. */
  async deal() {
    if (this.#busy) return;
    this.#busy = true;
    this.#setButtons([]);
    this.#say('Loading the table…');
    try {
      if (!this.#mod) await this.#load();
    } catch (err) {
      this.#fail(err);
      this.#busy = false;
      return;
    }
    const seed = this.hasAttribute('seed') ? Number(this.getAttribute('seed')) : Math.random();
    // The picture steps aside — it stays in the light DOM for rule 3.
    this.#root.querySelector('.pk-frame').classList.add('pk-live');
    this.#zone.hidden = false;
    this.#raise.hidden = true;
    this.#table?.clear?.();
    this.#state = JSON.parse(this.#mod.init_game(seed));
    this.#render(this.#state);
    this.#say('Dealing…');
    this.#busy = false;
    await this.#runBots();
  }

  // ── Loading ───────────────────────────────────────────────────────────────

  // Where pk-table.css is. In the release tarball everything is flat, so it
  // sits beside this module; a checkout keeps it under css/, hence `css`.
  get #cssUrl() {
    const c = this.getAttribute('css');
    return c ? new URL(c, document.baseURI) : new URL('pk-table.css', HERE);
  }

  get #base() {
    const b = this.getAttribute('base');
    return b ? new URL(b.endsWith('/') ? b : b + '/', document.baseURI) : HERE;
  }

  async #load() {
    const base = this.#base;
    // Rule 2: the module cache keys on the full URL, so a distinct query per
    // element yields a distinct module with its own thread_local! state —
    // the trick main.js plays with ?tab=play / ?tab=arena, written down.
    const glue = new URL(`pkarena0_web.js?instance=${this.#id}`, base);
    const [mod] = await Promise.all([
      import(glue.href).then(async m => {
        await m.default({ module_or_path: new URL('pkarena0_web_bg.wasm', base) });
        return m;
      }),
      this.#adoptSheet(this.#cssUrl),
    ]);
    this.#mod = mod;
    this.#table = createTable(this.#zone);
    this.#badge.textContent = 'pkarena0-web v' + mod.version();
    this.#emit('pk-table:ready', { version: mod.version() });
  }

  async #adoptSheet(url) {
    const key = url.href;
    if (!sheets.has(key)) {
      sheets.set(key, fetch(url).then(async r => {
        if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
        const sheet = new CSSStyleSheet();
        await sheet.replace(await r.text());
        return sheet;
      }));
    }
    const sheet = await sheets.get(key);
    this.#root.adoptedStyleSheets = [sheet];
  }

  // Rule 3: failure keeps the picture. The slot never moved, the zone stays
  // hidden, Deal is disabled, one sentence says why, and nothing spins.
  #fail(err) {
    const file = location.protocol === 'file:';
    const message = file
      ? 'The table needs to be served over http; from a file you get the picture.'
      : 'The table did not load; the picture stays. (' + (err?.message ?? err) + ')';
    this.#zone.hidden = true;
    this.#root.querySelector('.pk-frame').classList.remove('pk-live');
    this.#say(message);
    this.#showDeal(true);
    this.dataset.phase = 'error';
    this.#emit('pk-table:error', { message });
  }

  // ── The loop (rule 5), as main.js plays it ────────────────────────────────

  get #delay() {
    const d = Number(this.getAttribute('delay'));
    return Number.isFinite(d) && this.hasAttribute('delay') ? Math.max(0, d) : 1000;
  }

  async #runBots() {
    if (this.#busy) return;
    this.#busy = true;
    const mod = this.#mod;
    this.dataset.phase = 'BotsActing';
    while (this.isConnected && this.#mod === mod) {
      const result = JSON.parse(mod.step_bot());
      if (result.done) break;
      if (result.seat != null && result.action_label) {
        this.#say(`${result.name} ${result.action_label}`);
        this.#table.setActionLabel(result.seat, result.action_label);
      }
      this.#state = JSON.parse(mod.get_state());
      this.#render(this.#state);
      await sleep(this.#delay);
    }
    this.#busy = false;
    if (this.#mod !== mod) return;
    this.#state = JSON.parse(mod.get_state());
    this.#afterBots(this.#state);
  }

  #afterBots(state) {
    if (!state || state.phase === 'Error') {
      this.#say('Engine error: ' + (state?.error ?? 'unknown'));
      this.#showDeal();
      this.dataset.phase = 'error';
      return;
    }
    this.#render(state);
    if (state.phase === 'SessionOver' || busted(state)) {
      this.#say(state.phase === 'SessionOver'
        ? 'Session over. Final chips: $' + n(state.hero?.chips)
        : 'You are out of chips.');
      this.#showDeal();
      this.dataset.phase = 'SessionOver';
      return;
    }
    if (state.phase === 'HandComplete') {
      // Settle the hand: next_hand() pays the pot and reports who won, then
      // leaves the engine at the top of the next hand, waiting for Next hand.
      const next = JSON.parse(this.#mod.next_hand());
      this.#hands += 1;
      this.dataset.hands = String(this.#hands);
      const text = resultText(next) ?? 'Hand complete.';
      this.#say(text + (next.phase === 'SessionOver' ? ' — Session over.' : ''));
      this.#render({ ...state, hero: next.hero ?? state.hero, players: next.players ?? state.players }, { row: false });
      this.#emit('pk-table:hand-over', { state: next });
      this.#state = next;
      this.dataset.phase = 'HandComplete';
      if (next.phase === 'SessionOver' || busted(next)) { this.#showDeal(); return; }
      this.#setButtons([{ label: 'Next hand', cls: 'primary', act: 'next', onClick: () => this.#nextHand() }]);
      return;
    }
    // WaitingForHuman
    this.dataset.phase = 'WaitingForHuman';
    this.#say(`Hand #${state.hand_number} — ${state.street ?? 'Preflop'} — your turn.`);
    this.#renderRow(state);
  }

  #nextHand() {
    this.#table.clear?.();
    this.#raise.hidden = true;
    this.#render(this.#state);
    this.#say('Dealing…');
    this.#runBots();
  }

  #act(action, amount, state) {
    this.#raise.hidden = true;
    this.#setButtons([]);
    this.#table.setActionLabel(0, heroLabel(action, amount, state));
    const next = JSON.parse(this.#mod.human_action(JSON.stringify({ action, amount: amount ?? 0 })));
    if (next.error && next.phase === 'WaitingForHuman') {
      this.#say('Not allowed: ' + next.error);
      this.#state = next;
      this.#renderRow(next);
      return;
    }
    this.#state = next;
    this.#render(next);
    this.#runBots();
  }

  #undo() {
    const s = JSON.parse(this.#mod.undo_action());
    this.#state = s;
    this.#raise.hidden = true;
    this.#render(s);
    this.#say(`Undone — Hand #${s.hand_number} — ${s.street ?? 'Preflop'} — your turn.`);
    this.#renderRow(s);
  }

  // ── Rendering ─────────────────────────────────────────────────────────────

  #render(state, { row = false } = {}) {
    if (!this.#table || !state || state.phase === 'Error') return;
    this.#table.update({
      pot: state.pot ?? 0,
      board: state.board ?? [],
      smallBlind: state.small_blind ?? 0,
      bigBlind: state.big_blind ?? 0,
      seats: [state.hero, ...(state.players ?? [])].filter(Boolean),
      dealerSeat: state.dealer_seat ?? null,
      currentSeat: null,
      heroSeat: 0,
      heroTurn: state.phase === 'WaitingForHuman',
      allInAmounts: null,
      showHud: false,
      nameHref: null,
      emoji: null,
    });
    this.#hero.replaceChildren();
    const hero = state.hero;
    if (hero) {
      const cards = el('div', 'pk-hero-cards');
      for (const c of hero.hole_cards ?? []) { const node = makeCard(c, 'hero'); if (node) cards.append(node); }
      const pos = state.dealer_seat != null ? POS_TAGS[((0 - state.dealer_seat) % 9 + 9) % 9] : '';
      const bb = state.big_blind > 0 ? Math.round((hero.chips ?? 0) / state.big_blind) : 0;
      const title = el('div', 'pk-hero-title',
        `${hero.name || 'You'}${pos ? ' · ' + pos : ''} · $${n(hero.chips)} (${bb} BB)` +
        (state.to_call ? ` · to call $${n(state.to_call)}` : ''));
      this.#hero.append(cards, title);
    }
    if (row && state.phase === 'WaitingForHuman') this.#renderRow(state);
  }

  // Rule 6: the row is rebuilt from buttonsFor(state); focus follows the
  // button with the same data-act, else the first button.
  #renderRow(state) {
    const had = this.#root.activeElement?.dataset?.act ?? null;
    const btns = buttonsFor(state).map(b => ({
      label: b.label, cls: b.cls, act: actOf(b),
      onClick: () => {
        if (b.action === 'undo') return this.#undo();
        if (b.action === 'bet-open') return this.#openRaise(state, b.betAction);
        if (b.action === 'bet-direct') return this.#act(b.betAction, b.amount, state);
        if (b.action === 'allin') return this.#act('AllIn', b.amount, state);
        return this.#act(b.action, 0, state);
      },
    }));
    this.#setButtons(btns);
    const buttons = [...this.#actions.querySelectorAll('button')];
    (buttons.find(x => x.dataset.act === had) ?? buttons[0])?.focus({ preventScroll: true });
  }

  #openRaise(state, betAction) {
    const min = state.min_raise ?? state.big_blind ?? 0;
    const max = state.max_bet ?? state.hero?.chips ?? 0;
    this.#raise.replaceChildren();
    const label = el('span', null, `${betAction} to $${n(min)}`);
    const range = el('input'); range.type = 'range';
    range.min = String(min); range.max = String(max); range.step = String(state.big_blind || 50); range.value = String(min);
    range.setAttribute('aria-label', `${betAction} amount`);
    range.addEventListener('input', () => { label.textContent = `${betAction} to $${n(Number(range.value))}`; });
    const ok = el('button', 'primary', 'Confirm'); ok.dataset.act = 'confirm';
    ok.addEventListener('click', () => this.#act(betAction, Number(range.value), state));
    const cancel = el('button', null, 'Cancel'); cancel.dataset.act = 'cancel';
    cancel.addEventListener('click', () => { this.#raise.hidden = true; });
    this.#raise.append(label, range, ok, cancel);
    this.#raise.hidden = false;
    range.focus({ preventScroll: true });
  }

  #setButtons(btns) {
    this.#actions.replaceChildren();
    for (const b of btns) {
      const btn = el('button', b.cls || null, b.label);
      btn.dataset.act = b.act;
      if (b.disabled) btn.disabled = true;
      btn.addEventListener('click', b.onClick);
      this.#actions.append(btn);
    }
  }

  #showDeal(disabled = false) {
    this.#setButtons([{ label: this.#hands ? 'Deal again' : 'Deal', cls: 'primary pk-deal', act: 'deal', disabled, onClick: () => this.deal() }]);
    this.#dealBtn = this.#actions.querySelector('button');
  }

  #say(text) { this.#status.textContent = text; }

  #emit(type, detail) {
    this.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true }));
  }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
const sleep = ms => (ms > 0 ? new Promise(r => setTimeout(r, ms)) : Promise.resolve());
const n = v => (v ?? 0).toLocaleString('en-US');

function actOf(b) {
  if (b.action === 'bet-direct') return /min/i.test(b.label) ? 'min' : 'pot';
  return ACT_OF[b.action] ?? ACT_OF[b.betAction] ?? '';
}

function busted(state) {
  return (state?.hand_number ?? 0) >= 1 && state?.hero?.state === 'Out' && (state?.hero?.chips ?? null) === 0;
}

function heroLabel(action, amount, state) {
  if (action === 'Call') return 'Call $' + n(Math.min(state.to_call ?? 0, state.max_bet ?? 0));
  if (action === 'Bet' || action === 'Raise') return action + ' $' + n(amount);
  if (action === 'AllIn') return 'All-In $' + n(amount);
  return action;
}

// "You won $450 — Two Pair", from next_hand()'s last_result, as main.js words it.
function resultText(next) {
  const result = next?.last_result?.[0];
  if (!result) return null;
  const names = result.names ?? [];
  const split = names.length > 1;
  const who = split ? names.slice(0, -1).join(', ') + ' & ' + names.at(-1) : (names[0] ?? 'Unknown');
  const hand = result.hand ? ` — ${result.hand}` : '';
  return `${who} ${split ? 'split' : 'won'} $${n(result.amount)}${hand}`;
}

customElements.define('pk-table', PkTable);
