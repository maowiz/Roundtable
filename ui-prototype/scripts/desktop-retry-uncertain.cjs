// Opens the latest run from History and presses "Retry N uncertain calls", then waits for the run to finish.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const output = 'E:/round ribiom/output/ui/desktop';
(async () => {
  let browser;
  for (let i = 0; i < 30; i++) { try { browser = await chromium.connectOverCDP('http://127.0.0.1:9222'); break; } catch { await new Promise(r => setTimeout(r, 1000)); } }
  assert(browser, 'could not attach to WebView2');
  const page = browser.contexts()[0].pages()[0];
  await page.waitForSelector('h1');
  await page.setViewportSize({ width: 1440, height: 960 }).catch(() => {});
  const dismiss = async () => { while (await page.getByRole('button', { name: 'Dismiss notification' }).count()) { await page.getByRole('button', { name: 'Dismiss notification' }).first().click(); await page.waitForTimeout(100); } };
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await page.waitForSelector('.history-list.real button');
  await page.locator('.history-list.real button').first().click();
  await page.waitForSelector('.stage-strip', { timeout: 20000 });
  const notice = page.locator('.notice');
  await notice.waitFor({ timeout: 10000 });
  console.log('notice:', (await notice.innerText()).replace(/\n/g, ' '));
  const retry = notice.getByRole('button', { name: /Retry .* uncertain/ });
  if (await retry.count()) { await retry.click(); } else { await notice.getByRole('button', { name: 'Resume' }).click(); }
  await dismiss();
  await page.screenshot({ path: path.join(output, 'web-05-retrying.png') });
  const t0 = Date.now();
  let last = '';
  while (Date.now() - t0 < 20 * 60 * 1000) {
    const text = await page.locator('.live-bar-text').innerText().catch(() => '');
    if (text !== last) { console.log(new Date().toISOString().slice(11, 19), text); last = text; }
    if (/Completed|failed|cancelled|blocked|interrupted/i.test(text)) break;
    await page.waitForTimeout(5000);
  }
  if (!/Completed/.test(last)) { console.log('not completed:', last); await browser.close(); process.exit(2); }
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
