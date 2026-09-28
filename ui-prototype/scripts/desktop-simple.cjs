// Checks the simplified desktop app over WebView2 CDP (port 9222) without changing the user's team.
// RESUME="<question fragment>" also resumes that paused run from History and waits for it to finish.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const out = 'E:/round ribiom/output/ui/simple';
require('node:fs').mkdirSync(out, { recursive: true });
const shot = (page, name) => page.screenshot({ path: path.join(out, name + '.png') });

(async () => {
  let browser;
  for (let i = 0; i < 30; i++) { try { browser = await chromium.connectOverCDP('http://127.0.0.1:9222'); break; } catch { await new Promise(r => setTimeout(r, 1000)); } }
  assert(browser, 'could not attach to WebView2');
  const page = browser.contexts()[0].pages()[0];
  await page.setViewportSize({ width: 1440, height: 960 }).catch(() => {});
  const inv = (cmd, args) => page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args ?? {}]);
  try {
    await page.reload();
    await page.waitForSelector('.rt-agent:not(.rt-add)', { timeout: 30000 });
    await page.waitForTimeout(1500);

    // the app-wide rules now live inside each agent's own prompts
    const rules = await inv('get_system_prompts');
    assert(Object.values(rules.current).every(v => !v.trim()), 'no separate shared rules are added any more');
    const team = (await inv('list_panels'))[0].doc;
    for (const s of team.seats) {
      assert(s.promptAnswer.includes('STAGE: independent answer') && s.promptReview.includes('STAGE: peer review') && s.promptImprove.includes('STAGE: self-improvement'), `${s.label} holds its complete prompts`);
      assert.equal(s.promptAnswer.split('STAGE: independent answer').length, 2, `${s.label}: rules folded once, not twice`);
    }
    assert(team.synthesizer.prompt.includes('STAGE: final synthesis'));
    console.log('agents:', team.seats.map(s => s.label).join(', '));
    await shot(page, '10-ask');

    // agent panel: name, model, one System prompt per step, Advanced; nothing else
    await page.locator('.rt-agent:not(.rt-add)').first().click();
    await page.waitForSelector('.rt-drawer');
    assert.equal(await page.getByText('Show the full prompt').count(), 0, 'preview removed');
    assert.equal(await page.getByText('Common rules').count(), 0, 'common rules section removed');
    assert.equal(await page.getByText('Shared rules').count(), 0, 'shared rules section removed');
    assert.equal(await page.locator('.rt-drawer textarea').count(), 1, 'exactly one prompt box');
    for (const tab of ['Answer', 'Review', 'Improve']) {
      await page.getByRole('tab', { name: tab }).click();
      const text = await page.getByRole('textbox', { name: 'System prompt: ' + tab }).inputValue();
      assert(text.length > 300, tab + ' prompt is the full text');
    }
    await page.getByRole('tab', { name: 'Answer' }).click();
    await shot(page, '11-agent-system-prompt');
    await page.getByRole('button', { name: 'Done' }).click();

    // settings: keys, defaults, about; no system-prompts section
    await page.getByRole('button', { name: 'Settings' }).click();
    await page.waitForSelector('.rt-card');
    assert.equal(await page.getByText('System prompts').count(), 0, 'settings no longer has a prompts section');
    await shot(page, '12-settings');

    if (process.env.RESUME) {
      await page.getByRole('button', { name: 'History' }).click();
      const row = page.locator('.rt-list-row').filter({ hasText: process.env.RESUME }).first();
      await row.waitFor({ timeout: 15000 });
      await row.click();
      await page.waitForSelector('.rt-run-head');
      console.log('before:', (await page.locator('.rt-meters').innerText()).replace(/\n/g, ' | '));
      await page.getByRole('button', { name: /Resume|Retry/ }).click();
      const t0 = Date.now();
      let last = '', meters = '';
      while (Date.now() - t0 < 15 * 60 * 1000) {
        const s = await page.locator('.rt-run-head .rt-eyebrow').innerText().catch(() => '');
        const m = (await page.locator('.rt-meters').innerText().catch(() => '')).replace(/\n/g, ' | ');
        if (s !== last || m !== meters) { console.log(new Date().toISOString().slice(11, 19), s, '·', m); last = s; meters = m; }
        if (/Completed|Paused|Stopped|Failed/.test(s) && Date.now() - t0 > 8000) break;
        await page.waitForTimeout(3000);
      }
      assert(/Completed/.test(last), 'run completed: ' + last);
      await page.waitForSelector('.rt-final');
      await shot(page, '13-resumed-run-completed');
    }
    console.log('simple UI OK');
  } finally {
    await browser.close();
  }
})().catch(e => { console.error(e); process.exit(1); });
