// Theme + deck-color switching with localStorage persistence.
const THEME_KEY = 'pkarena.theme';
const DECK_KEY = 'pkarena.deck';
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

export function initDeckToggle() {
  // Implemented in the deck-toggle task.
}
