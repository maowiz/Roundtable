// Web search switches on the real app: edit the default panel, turn on the answer stage, check preflight shows the fee.
// Optional: RUN=1 starts a real run (about $0.20 with 4 seats) and waits for it, then checks sources on the answer cards.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const output = 'E:/round ribiom/output/ui/desktop';
require('node:fs').mkdirSync(output, { recursive: true });
const PANEL = 'Default test panel';
const QUESTION = 'What changed in the OpenRouter API in the last three months, and what should a small desktop app that calls it watch out for? Give a short, sourced answer.';
(async () => {
  let browser;
  for (let i = 0; i < 30; i++) { try { browser = await chromium.connectOverCDP('http://127.0.0.1:9222'); break; } catch { await new Promise(r => setTimeout(r, 1000)); } }
  assert(browser, 'could not attach to WebView2');
  const page = browser.contexts()[0].pages()[0];
  await page.waitForSelector('h1');
  await page.setViewportSize({ width: 1440, height: 960 }).catch(() => {});
  const dismiss = async () => { while (await page.getByRole('button', { name: 'Dismiss notification' }).count()) { await page.getByRole('button', { name: 'Dismiss notification' }).first().click(); await page.waitForTimeout(100); } };

  // 1. panel editor: the four switches, answers on
  await page.getByRole('button', { name: 'AI Panels', exact: true }).click();
  await page.waitForSelector('.real-panels');
  const card = page.locator('.panel-card').filter({ hasText: PANEL }).first();
  assert.equal(await card.count(), 1, 'default panel exists');
  await card.getByRole('button', { name: 'Edit' }).click();
  await page.waitForSelector('.draft-web');
  const rows = page.locator('.web-row');
  assert.equal(await rows.count(), 4, 'four stage switches');
  for (let i = 0; i < 4; i++) { const cb = rows.nth(i).locator('input'); if (await cb.isChecked()) await cb.click(); }
  await rows.nth(0).locator('input').click();
  assert((await page.locator('.draft-web .muted.small').innerText()).includes('4 searches per run'), 'fee line for 4 answer calls');
  await page.locator('.draft-web').scrollIntoViewIfNeeded();
  await page.screenshot({ path: path.join(output, 'web-01-panel-switches.png') });
  await page.getByRole('button', { name: 'Save panel' }).click();
  await page.waitForSelector('.panel-draft', { state: 'detached' });
  await dismiss();
  // it persisted
  await card.getByRole('button', { name: 'Edit' }).click();
  await page.waitForSelector('.draft-web');
  assert.equal(await page.locator('.web-row').nth(0).locator('input').isChecked(), true, 'answer switch persisted');
  assert.equal(await page.locator('.web-row').nth(1).locator('input').isChecked(), false);
  await page.getByRole('button', { name: 'Discard' }).click();

  // 2. home: preflight shows the search calls and fee inside the estimate
  await page.getByRole('button', { name: 'Home', exact: true }).click();
  await page.waitForSelector('.real-picker');
  await page.waitForSelector('#question');
  await page.locator('#question').fill(QUESTION);
  await page.getByRole('button', { name: 'Run options' }).click();
  await page.getByRole('spinbutton').fill('1.50');
  await page.getByRole('button', { name: 'Save options' }).click();
  await dismiss();
  await page.waitForSelector('.real-picker select');
  const options = await page.locator('.real-picker select option').allTextContents();
  await page.locator('.real-picker select').selectOption({ index: options.findIndex(o => o.includes(PANEL)) });
  await page.waitForSelector('.preflight .web-note', { timeout: 20000 });
  const note = await page.locator('.preflight .web-note').innerText();
  console.log('preflight:', (await page.locator('.preflight').innerText()).replace(/\n/g, ' | '));
  assert(note.includes('Web search on 4 calls'), note);
  await page.screenshot({ path: path.join(output, 'web-02-preflight.png') });

  if (process.env.RUN !== '1') { console.log('web search switches OK (no run started)'); await browser.close(); return; }

  // 3. a real run with search on the answer stage only
  await page.getByRole('button', { name: 'Start roundtable' }).click();
  await page.waitForSelector('.live-page', { timeout: 30000 });
  await dismiss();
  await page.waitForSelector('.live-bar', { timeout: 60000 });
  const t0 = Date.now();
  let last = '';
  while (Date.now() - t0 < 15 * 60 * 1000) {
    const text = await page.locator('.live-bar-text').innerText().catch(() => '');
    if (text !== last) { console.log(new Date().toISOString().slice(11, 19), text); last = text; }
    if (/Completed|failed|cancelled|blocked|interrupted/i.test(text)) break;
    await page.waitForTimeout(5000);
  }
  assert(/Completed/.test(last), 'run completed: ' + last);
  // stage 1 box carries the web badge; the answer cards list their sources
  assert.equal(await page.locator('.stage-strip button').nth(0).locator('.stage-web').count(), 1, 'answers stage shows the web badge');
  assert.equal(await page.locator('.stage-strip button').nth(1).locator('.stage-web').count(), 0, 'reviews stage has no badge');
  await page.locator('.stage-strip button').nth(0).click();
  await page.waitForSelector('.stage-docs');
  await page.getByRole('button', { name: 'Open all' }).click();
  const sources = await page.locator('.doc-sources li').count();
  const searched = await page.locator('.doc-title small:has-text("searched the web")').count();
  console.log(`answer cards that searched: ${searched} of 4 · sources listed: ${sources}`);
  await page.screenshot({ path: path.join(output, 'web-03-answers-with-sources.png') });
  await page.locator('.doc-sources').first().scrollIntoViewIfNeeded().catch(() => {});
  await page.screenshot({ path: path.join(output, 'web-04-sources.png') });
  console.log('spent:', await page.locator('.live-status').innerText().catch(() => '?'));
  console.log('web search run OK');
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
