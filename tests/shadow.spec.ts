import { test, expect } from '@playwright/test';

// EPIC-56 Phase 0a: the embed will mount createTable inside a shadow root so
// the host page's CSS cannot reach it. This pins the two things that makes
// possible: table.js never looks anything up on `document`, and the table's
// own stylesheet applies when linked from inside the root.
test.describe('createTable inside a shadow root', () => {
  test('builds, updates, and is styled by a stylesheet inside the root', async ({ page }) => {
    await page.goto('/');
    const got = await page.evaluate(async () => {
      const { createTable } = await import('/js/table.js');
      const host = document.createElement('div');
      host.id = 'shadow-host';
      host.style.cssText = 'position:relative;width:1000px;height:600px';
      document.body.appendChild(host);
      const root = host.attachShadow({ mode: 'open' });
      const link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = '/css/table.css';
      const loaded = new Promise(r => { link.onload = r; link.onerror = r; });
      root.append(link);
      const mount = document.createElement('div');
      root.append(mount);
      const t = createTable(mount);
      t.update({
        pot: 1500, board: ['Kh', 'Td', '2s'], smallBlind: 50, bigBlind: 100,
        seats: [
          { seat: 0, name: 'You', chips: 10000, bet: 0, state: 'Active',
            hole_cards: ['As', 'Ad'], is_dealer: true, is_sb: false, is_bb: false },
          { seat: 1, name: 'gto', chips: 9950, bet: 50, state: 'Active',
            hole_cards: ['__', '__'], is_dealer: false, is_sb: true, is_bb: false },
        ],
        dealerSeat: 0, currentSeat: null, heroSeat: 0, heroTurn: true,
        allInAmounts: null, showHud: false, nameHref: null, emoji: null,
      });
      await loaded;
      const felt = root.querySelector('.felt') as HTMLElement;
      return {
        seats: root.querySelectorAll('[data-seat]').length,
        slots: root.querySelectorAll('.board-slot').length,
        boardCards: root.querySelectorAll('.board-slot .card').length,
        pot: root.querySelector('.pot')?.textContent,
        // Nothing leaked into the host's light DOM (the page's own table
        // lives elsewhere in the document, so count under the host).
        lightSeats: host.querySelectorAll('[data-seat]').length,
        // table.css reached the felt: it sets position:absolute on .felt.
        feltPosition: felt ? getComputedStyle(felt).position : null,
      };
    });
    expect(got.seats).toBe(9);
    expect(got.slots).toBe(5);
    expect(got.boardCards).toBe(3);
    expect(got.pot).toContain('1,500');
    expect(got.lightSeats).toBe(0);
    expect(got.feltPosition).toBe('absolute');
  });
});
