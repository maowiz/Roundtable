// Opens the most recent completed run from History and walks the four stage pages (real ledger papers).
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const output = 'E:/round ribiom/output/ui/desktop';
require('node:fs').mkdirSync(output, { recursive: true });
(async () => {
  let browser;
  for (let i = 0; i < 30; i++) { try { browser = await chromium.connectOverCDP('http://127.0.0.1:9222'); break; } catch { await new Promise(r => setTimeout(r, 1000)); } }
  assert(browser, 'could not attach to WebView2');
  const page = browser.contexts()[0].pages()[0];
  await page.waitForSelector('h1');
  await page.setViewportSize({ width: 1440, height: 960 }).catch(() => {});
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await page.waitForSelector('.history-list.real button');
  await page.locator('.history-list.real button').first().click();
  await page.waitForSelector('.run-result, .result', { timeout: 15000 }).catch(() => {});
  await page.screenshot({ path: path.join(output, 'stages-00-result.png') });
  await page.getByRole('button', { name: /How we got here/ }).click();
  await page.waitForSelector('.stage-strip');
  assert.equal(await page.locator('input[aria-label="Timeline"]').count(), 0, 'no timeline slider');
  assert.equal(await page.locator('.play-button').count(), 0, 'no play button');
  await page.waitForSelector('.live-bar');
  await page.screenshot({ path: path.join(output, 'stages-01-table.png') });
  const names = ['Independent answers', 'Peer reviews', 'Self-improvement', 'Final synthesis'];
  for (let i = 0; i < 4; i++) {
    await page.locator('.stage-strip button').nth(i).click();
    await page.waitForSelector('.stage-docs');
    const head = await page.locator('.stage-docs-head h2').innerText();
    assert.equal(head, names[i]);
    const cards = await page.locator('.doc-card').count();
    const count = await page.locator('.doc-count').innerText();
    console.log(`stage ${i + 1}: ${head} · ${cards} cards · ${count}`);
    if (i === 1) {
      const labels = await page.locator('.doc-card .doc-title').allInnerTexts();
      console.log(labels.map(l => l.replace(/\n/g, ' | ')).join('\n'));
      assert.equal(cards, 12, '12 reviews for 4 seats');
    }
    // no scrollbars inside the page: the surface and the cards must not scroll on their own
    const inner = await page.evaluate(() => [...document.querySelectorAll('.stage-docs, .stage-docs *')].filter(e => { const cs = getComputedStyle(e); return /(auto|scroll)/.test(cs.overflowY) && e.scrollHeight > e.clientHeight; }).length);
    assert.equal(inner, 0, 'no nested scrollbars inside the papers page');
    assert.equal(await page.locator('.activity-panel').count(), 0, 'side panel hidden while reading');
    await page.screenshot({ path: path.join(output, `stages-0${i + 2}-${names[i].toLowerCase().replace(/\s+/g, '-')}.png`), fullPage: false });
    // cards open and close in place
    if (i < 3) {
      const first = page.locator('.doc-card').first();
      assert.equal(await first.locator('.doc-body').count(), 0, 'card starts closed');
      await first.locator('.doc-head').click();
      await page.waitForSelector('.doc-card.open .doc-body');
      assert.equal(await page.locator('.doc-card.open .md p, .doc-card.open .doc-issue, .doc-card.open .doc-ok').count() > 0, true, 'body rendered');
      assert.equal(await page.locator('.doc-card.open .md').evaluate(el => /\*\*|^#+\s/m.test(el.innerText)).catch(() => false), false, 'markdown rendered, no raw markers');
      await page.screenshot({ path: path.join(output, `stages-0${i + 2}b-open.png`), fullPage: false });
      await first.locator('.doc-head').click();
      assert.equal(await first.locator('.doc-body').count(), 0, 'card closed again');
    } else {
      assert.equal(await page.locator('.doc-card.open').count(), 1, 'final answer opens by itself');
    }
  }
  // Previous/Next buttons walk the stages
  await page.locator('.stage-nav button').first().click();
  assert.equal(await page.locator('.stage-docs-head h2').innerText(), 'Self-improvement');
  await page.locator('.stage-nav button').last().click();
  assert.equal(await page.locator('.stage-docs-head h2').innerText(), 'Final synthesis');
  // the review page scrolled so a reviewer can see the labels of the second group
  await page.locator('.stage-strip button').nth(1).click();
  await page.getByRole('button', { name: 'Open all' }).click();
  await page.locator('.doc-group').nth(1).scrollIntoViewIfNeeded();
  await page.screenshot({ path: path.join(output, 'stages-03b-peer-reviews-group2.png') });
  console.log('stage browser OK');
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
