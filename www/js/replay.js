// Replay overlay — renders session/upload snapshots through its own table
// instance (createTable with { replay: true }, so every seat shows hole cards).
import { createTable } from './table.js';

let replayTable = null;
let getMod = null;
const replay = { yamlText: null, hands: [], handIndex: 0, step: 0, totalSteps: 0, source: null };

function ensureReplayTable() {
  if (!replayTable) {
    replayTable = createTable(document.getElementById('replay-table-wrapper'), { replay: true });
  }
  return replayTable;
}

function buildReplayView(snap) {
  return {
    pot: snap.pot ?? 0,
    board: snap.board ?? [],
    smallBlind: 0, bigBlind: 0,           // replay center label stays generic
    seats: snap.seats ?? [],
    dealerSeat: (snap.seats ?? []).find(s => s.is_dealer)?.seat ?? null,
    currentSeat: snap.current_seat ?? null,
    heroSeat: null, heroTurn: false,
    allInAmounts: null,                   // table.update falls back to p.bet for AllIn
    showHud: false, nameHref: null, emoji: null,
  };
}

function renderReplayTable(snap) {
  ensureReplayTable().update(buildReplayView(snap));
  document.getElementById('replay-action-label').textContent = snap.action_label ?? '—';
  document.getElementById('replay-step-counter').textContent =
    (snap.step + 1) + ' / ' + snap.total_steps;
  const slider = document.getElementById('replay-slider');
  slider.max = Math.max(0, (snap.total_steps ?? 1) - 1);
  slider.value = snap.step ?? 0;
  slider.disabled = (snap.total_steps ?? 0) <= 1;
  document.getElementById('replay-prev').disabled = (snap.step ?? 0) <= 0;
  document.getElementById('replay-next').disabled = (snap.step ?? 0) >= (snap.total_steps - 1);
}

function showReplayError(msg) {
  document.getElementById('replay-action-label').textContent = msg;
  document.getElementById('replay-step-counter').textContent = '0 / 0';
  const slider = document.getElementById('replay-slider');
  slider.max = 0; slider.value = 0; slider.disabled = true;
  document.getElementById('replay-prev').disabled = true;
  document.getElementById('replay-next').disabled = true;
}

function loadReplayCollection(yamlText, sourceLabel) {
  const mod = getMod();
  if (!mod) { showReplayError('No game module loaded yet.'); return; }
  let summary;
  try {
    summary = JSON.parse(mod.parse_hand_collection(yamlText));
  } catch (e) {
    showReplayError('Failed to parse YAML: ' + e.message);
    return;
  }
  if (summary.error) { showReplayError(summary.error); return; }
  const hands = summary.hands ?? [];
  replay.yamlText = yamlText;
  replay.hands    = hands;
  replay.handIndex = 0;
  replay.step      = 0;
  replay.totalSteps = 0;
  replay.source    = sourceLabel === 'Session' ? 'session' : 'upload';

  const picker = document.getElementById('replay-hand-picker');
  while (picker.firstChild) picker.removeChild(picker.firstChild);
  if (hands.length === 0) {
    const opt = document.createElement('option');
    opt.value = ''; opt.textContent = sourceLabel + ' has no hands yet';
    picker.appendChild(opt);
    picker.disabled = true;
    showReplayError('No hands available to replay.');
    return;
  }
  for (const h of hands) {
    const opt = document.createElement('option');
    opt.value = String(h.index);
    opt.textContent = h.description;
    picker.appendChild(opt);
  }
  picker.disabled = false;
  picker.value = '0';
  selectReplayHand(0);
}

function selectReplayHand(handIndex) {
  replay.handIndex = handIndex;
  replay.step = 0;
  renderReplayStep(0);
}

function renderReplayStep(step) {
  const mod = getMod();
  if (!mod || replay.yamlText == null) return;
  let snap;
  try {
    snap = JSON.parse(mod.replay_snapshot(replay.yamlText, replay.handIndex, step));
  } catch (e) {
    showReplayError('Replay error: ' + e.message);
    return;
  }
  if (snap.error) { showReplayError(snap.error); return; }
  replay.step = snap.step;
  replay.totalSteps = snap.total_steps;
  renderReplayTable(snap);
}

// Wires every listener inside #replay-overlay plus the #btn-replay open button.
export function initReplay(opts) {
  getMod = opts.getMod;
  const replayOverlay = document.getElementById('replay-overlay');

  document.getElementById('btn-replay').addEventListener('click', () => {
    ensureReplayTable();
    replayOverlay.classList.add('open');
    // Refresh session data on every open, but leave a user-uploaded YAML intact.
    if (replay.source !== 'upload') {
      const mod = getMod();
      if (mod) {
        const yaml = mod.get_session_yaml();
        loadReplayCollection(yaml, 'Session');
      }
    }
  });

  document.getElementById('replay-close').addEventListener('click', () => {
    replayOverlay.classList.remove('open');
  });

  replayOverlay.addEventListener('click', (e) => {
    if (e.target === replayOverlay) replayOverlay.classList.remove('open');
  });

  document.getElementById('replay-load-session').addEventListener('click', () => {
    const mod = getMod();
    if (!mod) { showReplayError('No game module loaded yet.'); return; }
    const yaml = mod.get_session_yaml();
    loadReplayCollection(yaml, 'Session');
  });

  document.getElementById('replay-file-input').addEventListener('change', (e) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => loadReplayCollection(String(reader.result), file.name);
    reader.onerror = () => showReplayError('Failed to read file.');
    reader.readAsText(file);
    // Reset so picking the same file again re-fires `change`.
    e.target.value = '';
  });

  document.getElementById('replay-hand-picker').addEventListener('change', (e) => {
    const idx = parseInt(e.target.value, 10);
    if (!Number.isFinite(idx)) return;
    selectReplayHand(idx);
  });

  document.getElementById('replay-prev').addEventListener('click', () => {
    if (replay.step > 0) renderReplayStep(replay.step - 1);
  });

  document.getElementById('replay-next').addEventListener('click', () => {
    if (replay.step < replay.totalSteps - 1) renderReplayStep(replay.step + 1);
  });

  document.getElementById('replay-slider').addEventListener('input', (e) => {
    const val = parseInt(e.target.value, 10);
    if (Number.isFinite(val) && val !== replay.step) renderReplayStep(val);
  });
}
