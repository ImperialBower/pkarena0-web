// Theme + deck-color switching with localStorage persistence.
const THEME_KEY = 'pkarena.theme';
const DECK_KEY = 'pkarena.deck';
const SEATS_KEY = 'pkarena.mobileSeats';
const THEMES = ['midnight', 'terminal', 'luxe', 'organic'];

function applyTheme(name) {
  for (const t of THEMES) document.body.classList.remove('theme-' + t);
  document.body.classList.add('theme-' + name);
}

export function initThemes() {
  const sel = document.getElementById('theme-select');
  let saved = null;
  try { saved = localStorage.getItem(THEME_KEY); } catch { /* private mode */ }
  const theme = THEMES.includes(saved) ? saved : 'midnight';
  applyTheme(theme);
  sel.value = theme;
  sel.addEventListener('change', () => {
    applyTheme(sel.value);
    try { localStorage.setItem(THEME_KEY, sel.value); } catch { /* ignore */ }
  });
}

export function initViewToggle() {
  const btn = document.getElementById('view-toggle');
  if (!btn) return;
  let saved = null;
  try { saved = localStorage.getItem(SEATS_KEY); } catch { /* ignore */ }
  const apply = mode => {
    const list = mode === 'list';
    document.body.classList.toggle('seats-list', list);
    btn.textContent = list ? 'TABLE ⇄' : 'LIST ⇄';   // label = what you switch TO
  };
  apply(saved === 'list' ? 'list' : 'table');
  btn.addEventListener('click', () => {
    const next = document.body.classList.contains('seats-list') ? 'table' : 'list';
    apply(next);
    try { localStorage.setItem(SEATS_KEY, next); } catch { /* ignore */ }
  });
}

export function initDeckToggle() {
  const btn = document.getElementById('deck-toggle');
  let saved = null;
  try { saved = localStorage.getItem(DECK_KEY); } catch { /* ignore */ }
  const apply = four => {
    document.body.classList.toggle('four-color', four);
    btn.textContent = four ? '4-COLOR ●' : '2-COLOR ○';
  };
  apply(saved === '4');
  btn.addEventListener('click', () => {
    const four = !document.body.classList.contains('four-color');
    apply(four);
    try { localStorage.setItem(DECK_KEY, four ? '4' : '2'); } catch { /* ignore */ }
  });
}
