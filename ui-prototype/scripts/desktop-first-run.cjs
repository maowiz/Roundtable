// Builds the default OpenRouter panel in the real app and runs the first real roundtable with a low ceiling.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const output = 'E:/round ribiom/output/ui/desktop';
fs.mkdirSync(output, { recursive: true });
const SEATS = ['google/gemini-3.8-flash', 'z-ai/glm-5.3-flash', 'deepseek/deepseek-v4-pro-0813', 'qwen/qwen3.8-max-0902'];
const WRITER = 'google/gemini-3.8-flash';
const QUESTION = 'How should a three-person startup launch a B2B SaaS product in 90 days with a $15,000 budget? Give a concrete plan with the biggest risks.';
const CEILING = '1.50';

(async () => {
  let browser;
  for (let i = 0; i < 30; i++) { try { browser = await chromium.connectOverCDP('http://127.0.0.1:9222'); break; } catch { await new Promise(r => setTimeout(r, 1000)); } }
  assert(browser, 'could not attach');
  const page = browser.contexts()[0].pages()[0];
  await page.waitForSelector('h1');
  const dismiss = async () => { while (await page.getByRole('button', { name: 'Dismiss notification' }).count()) { await page.getByRole('button', { name: 'Dismiss notification' }).click(); await page.waitForTimeout(150); } };

  // ---- panel
  await page.getByRole('button', { name: 'AI Panels', exact: true }).click();
  await page.waitForSelector('.real-panels');
  const existing = page.locator('.panel-card').filter({ hasText: 'Default test panel' });
  if (await existing.count()) {
    await existing.first().getByRole('button', { name: /Delete/ }).click();
    await page.waitForTimeout(500);
    console.log('replaced existing panel');
  }
  {
    await page.getByRole('button', { name: 'New real panel' }).click();
    await page.waitForSelector('.panel-draft');
    await page.locator('.panel-draft > .field-label input').first().fill('Default test panel');
    const seats = page.locator('.draft-seat');
    assert.equal(await seats.count(), 4);
    for (let i = 0; i < 4; i++) {
      const seat = seats.nth(i);
      await seat.locator('select').nth(1).selectOption(SEATS[i]);
      await seat.locator('input[type="number"]').fill('700');
      await seat.locator('.draft-grid input').nth(1).fill('0.50');
    }
    await page.locator('.draft-synth select').nth(1).selectOption(WRITER);
    await page.screenshot({ path: path.join(output, 'run-01-panel-draft.png') });
    await page.getByRole('button', { name: 'Save panel' }).click();
    await page.waitForSelector('.panel-card');
    await dismiss();
  }
  await page.screenshot({ path: path.join(output, 'run-02-panels.png') });

  // ---- home: question, panel, ceiling
  await page.getByRole('button', { name: 'Home', exact: true }).click();
  await page.waitForSelector('#question');
  await page.locator('#question').fill(QUESTION);
  await page.getByRole('button', { name: 'Run options' }).click();
  await page.getByRole('spinbutton').fill(CEILING);
  await page.getByRole('button', { name: 'Save options' }).click();
  await dismiss();
  await page.waitForSelector('.real-picker select');
  const opts = await page.locator('.real-picker select option').allTextContents();
  const idx = opts.findIndex(o => o.includes('Default test panel'));
  assert(idx >= 0, 'panel listed: ' + opts.join('|'));
  await page.locator('.real-picker select').selectOption({ index: idx });
  await page.waitForSelector('.preflight');
  await page.waitForTimeout(800);
  const preflight = await page.locator('.preflight').innerText();
  console.log('PREFLIGHT:', preflight.replace(/\n/g, ' | '));
  await page.screenshot({ path: path.join(output, 'run-03-preflight.png') });
  assert(preflight.includes('Ready'), 'preflight not ready');

  // ---- start
  await page.getByRole('button', { name: 'Start roundtable' }).click();
  await page.waitForSelector('.live-page', { timeout: 30000 });
  await dismiss();
  const started = Date.now();
  let lastState = '';
  let midShot = false;
  while (Date.now() - started < 16 * 60 * 1000) {
    await page.waitForTimeout(3000);
    const strip = await page.locator('.stage-strip').innerText().catch(() => '');
    const status = await page.locator('.surface-heading > span').first().innerText().catch(() => '');
    const bar = await page.locator('.live-bar-text').innerText().catch(() => '');
    if (bar && bar !== lastState) { console.log(bar); lastState = bar; continue; }
    const money = await page.locator('.session-bottom .live-status').innerText().catch(() => '');
    const line = `${Math.round((Date.now() - started) / 1000)}s ${status} | ${money.replace(/\n/g, ' ')}`;
    if (line !== lastState) { console.log(line); lastState = line; }
    if (!midShot && strip.includes('Peer reviews') && status.includes('Live')) { await page.screenshot({ path: path.join(output, 'run-04-live.png') }); midShot = true; }
    if (await page.getByRole('button', { name: 'Open the final answer' }).first().isEnabled().catch(() => false)) break;
    if (/blocked|failed|cancelled/i.test(status)) break;
  }
  await page.screenshot({ path: path.join(output, 'run-05-live-end.png') });
  const finalBtn = page.getByRole('button', { name: 'Open the final answer' }).first();
  if (await finalBtn.isEnabled().catch(() => false)) {
    await finalBtn.click();
    await page.waitForSelector('.answer-paper.real', { timeout: 20000 });
    await page.waitForTimeout(500);
    await page.screenshot({ path: path.join(output, 'run-06-result.png'), fullPage: false });
    const subtitle = await page.locator('.page-heading .subtitle').innerText();
    console.log('RESULT:', subtitle);
    const answer = await page.locator('.answer-body').innerText();
    fs.writeFileSync(path.join(output, 'run-final-answer.txt'), answer);
    console.log('ANSWER (first 600 chars):', answer.slice(0, 600).replace(/\n/g, ' '));
  } else {
    const notice = await page.locator('.notice').innerText().catch(() => 'no notice');
    console.log('RUN DID NOT COMPLETE:', notice);
  }
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
