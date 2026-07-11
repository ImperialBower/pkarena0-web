import { test, expect } from '@playwright/test';

test.describe('createTable component', () => {
  test('builds 9 seats, 5 board slots, pot; update() populates a seat', async ({ page }) => {
    await page.goto('/');
    const counts = await page.evaluate(async () => {
      const { createTable } = await import('/js/table.js');
      const el = document.createElement('div');
      el.id = 'tbl-test';
      el.style.cssText = 'position:relative;width:1000px;height:600px';
      document.body.appendChild(el);
      const t = createTable(el);
      t.update({
        pot: 1500, board: ['Kh', 'Td', '2s'], smallBlind: 50, bigBlind: 100,
        seats: [
          { seat: 0, name: 'You', chips: 10000, bet: 0, state: 'Active',
            hole_cards: null, is_dealer: true, is_sb: false, is_bb: false },
          { seat: 1, name: 'gto', chips: 9950, bet: 50, state: 'Active',
            hole_cards: ['__', '__'], is_dealer: false, is_sb: true, is_bb: false },
        ],
        dealerSeat: 0, currentSeat: null, heroSeat: 0, heroTurn: true,
        allInAmounts: null, showHud: false, nameHref: null, emoji: null,
      });
      return {
        seats: el.querySelectorAll('[data-seat]').length,
        slots: el.querySelectorAll('.board-slot').length,
        boardCards: el.querySelectorAll('.board-slot .card').length,
        pot: el.querySelector('.pot')?.textContent,
        name0: el.querySelector('[data-seat="0"] .seat-name')?.textContent,
        tag1: el.querySelector('[data-seat="1"] .seat-tag')?.textContent,
        stack1: el.querySelector('[data-seat="1"] .seat-stack')?.textContent,
        bet1: el.querySelector('[data-seat="1"] .seat-chipamt')?.textContent,
        heroRing: el.querySelector('[data-seat="0"] .seat-ring')
          ?.classList.contains('hero-ring'),
      };
    });
    expect(counts.seats).toBe(9);
    expect(counts.slots).toBe(5);
    expect(counts.boardCards).toBe(3);
    expect(counts.pot).toContain('1,500');
    expect(counts.name0).toBe('You');
    expect(counts.tag1).toBe('SB');
    expect(counts.stack1).toBe('$9,950');
    expect(counts.bet1).toBe('$50');
    expect(counts.heroRing).toBe(true);
  });
});
