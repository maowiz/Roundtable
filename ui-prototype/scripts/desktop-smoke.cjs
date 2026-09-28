// Drives the real Tauri app through WebView2's remote-debugging port (real commands, real Credential Manager).
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const output = 'E:/round ribiom/output/ui/desktop';
require('node:fs').mkdirSync(output, { recursive: true });
(async () => {
  let browser;
  for (let i = 0; i < 30; i++) {
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:9222'); break; } catch { await new Promise(r => setTimeout(r, 1000)); }
  }
  assert(browser, 'could not attach to WebView2');
  const page = browser.contexts()[0].pages()[0];
  await page.waitForSelector('h1');
  const desktop = await page.evaluate(() => !!window.__TAURI_INTERNALS__);
  assert(desktop, 'Tauri internals present');
  await page.setViewportSize({ width: 1440, height: 960 }).catch(() => {});

  // Settings → Connections is the real credential manager
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: 'Connections', exact: true }).click();
  await page.waitForSelector('.connections.real');
  await page.locator('.add-key select').selectOption('openai');
  await page.locator('.add-key input[type="password"]').fill('sk-smoke-not-a-real-key-0123456789');
  await page.getByRole('button', { name: 'Save key' }).click();
  await page.waitForSelector('.connection-row strong:has-text("OpenAI")');
  const row = page.locator('.connection-row').filter({ hasText: 'OpenAI' }).first();
  assert((await row.innerText()).includes('key present'), 'Credential Manager reports the key');
  await page.screenshot({ path: path.join(output, 'desktop-01-connections.png') });
  if (await page.getByRole('button', { name: 'Dismiss notification' }).count()) await page.getByRole('button', { name: 'Dismiss notification' }).click();
  await page.waitForSelector('.toast', { state: 'detached' });
  // real connection test against api.openai.com with a bogus key: expect a clean auth failure
  await row.getByRole('button', { name: 'Test' }).click();
  await page.waitForSelector('.toast', { timeout: 90000 });
  const toast = await page.locator('.toast').innerText();
  assert(/auth|401|invalid|incorrect|failed|unauthori/i.test(toast), 'expected an auth-category failure, got: ' + toast);
  console.log('test result toast:', toast);
  assert(!toast.includes('sk-smoke'), 'secret never appears in messages');
  await page.screenshot({ path: path.join(output, 'desktop-02-test-result.png') });
  await page.getByRole('button', { name: 'Dismiss notification' }).click();
  // clean up the credential
  await row.getByRole('button', { name: /Remove/ }).click();
  await page.waitForSelector('.connections.real p.muted');

  // AI Panels shows the real panel section; Home shows the real picker
  await page.getByRole('button', { name: 'AI Panels', exact: true }).click();
  await page.waitForSelector('.real-panels');
  await page.getByRole('button', { name: 'New real panel' }).click();
  await page.waitForSelector('.toast');
  assert((await page.locator('.toast').innerText()).includes('at least two'));
  await page.screenshot({ path: path.join(output, 'desktop-03-panels.png') });
  await page.getByRole('button', { name: 'Dismiss notification' }).click();
  await page.getByRole('button', { name: 'Home', exact: true }).click();
  await page.waitForSelector('.real-picker');
  await page.screenshot({ path: path.join(output, 'desktop-04-home.png') });
  // History is the real ledger (empty)
  await page.getByRole('button', { name: 'History', exact: true }).click();
  await page.waitForSelector('.history-search');
  assert.equal(await page.locator('.empty-state').count(), 1);
  const info = await page.evaluate(async () => { const { invoke } = window.__TAURI__?.core ?? {}; return invoke ? invoke('app_info') : 'no global'; });
  console.log('app_info via IPC:', JSON.stringify(info));
  console.log('PASS: desktop IPC, Credential Manager save/test/delete, real panels, picker, history');
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
