// Card element factory. cardStr is 2-char ("Kh") or "__" for face-down.
const SUIT_CHAR = { s: '♠', h: '♥', d: '♦', c: '♣' };

export function makeCard(cardStr, size = 'board') {
  if (!cardStr) return null;
  const el = document.createElement('div');
  el.className = 'card card-' + size;
  if (cardStr === '__') {
    el.classList.add('card-down');
    return el;
  }
  const suit = cardStr[cardStr.length - 1];
  const rank = cardStr.slice(0, -1);
  el.classList.add('suit-' + suit);
  const r = document.createElement('span');
  r.className = 'card-rank';
  r.textContent = rank === 'T' ? '10' : rank;
  const s = document.createElement('span');
  s.className = 'card-suit';
  s.textContent = SUIT_CHAR[suit] ?? '?';
  el.append(r, s);
  return el;
}
