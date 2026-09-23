// End-to-end test of the Aurion web interface in a real browser.
//
//   cargo build && cd tests/e2e && npm install && npm test
//
// Starts `aurion serve` on a temporary config + fake USB drive, then drives
// every page with Chromium: no JavaScript error, settings saved and
// persisted, presets, gallery (lightbox, sessions, deletion), diagnostics.
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../..');
const binary = process.env.AURION_BIN || path.join(repo, 'target/debug/aurion');
const port = 18000 + Math.floor(Math.random() * 1000);
const base = `http://127.0.0.1:${port}`;

// ─── Fixture: config dir + fake USB drive with one night ────
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'aurion-e2e-'));
const capture = path.join(work, 'capture');
const configDir = path.join(work, 'config');
fs.mkdirSync(path.join(capture, 'sessions/2026-03-05_21-30'), { recursive: true });
fs.mkdirSync(path.join(capture, 'thumbs'), { recursive: true });
fs.mkdirSync(configDir, { recursive: true });
const cfg = JSON.parse(fs.readFileSync(path.join(repo, 'config/default.json'), 'utf8'));
cfg.storage.mount_point = capture;
cfg.web.port = port;
fs.writeFileSync(path.join(configDir, 'aurion.json'), JSON.stringify(cfg, null, 2));
fs.writeFileSync(path.join(capture, 'sessions/2026-03-05_21-30/event.jsonl'), '{"aurora_detected":true}\n');

// ─── Server ─────────────────────────────────────────────────
const server = spawn(binary, ['--config-dir', configDir, 'serve', '--port', String(port)], {
  stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, RUST_LOG: 'warn' },
});
let serverLog = '';
server.stdout.on('data', d => { serverLog += d; });
server.stderr.on('data', d => { serverLog += d; });

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try { if ((await fetch(base + '/api/status')).ok) return; } catch (_) { /* not yet */ }
    await new Promise(r => setTimeout(r, 100));
  }
  throw new Error('server did not start:\n' + serverLog);
}

// ─── Test helpers ───────────────────────────────────────────
let failures = 0;
async function step(name, fn) {
  try { await fn(); console.log('  ok  ' + name); }
  catch (e) { failures++; console.log('  FAIL ' + name + '\n       ' + (e.stack || e).toString().split('\n').slice(0, 3).join('\n       ')); }
}
function assert(cond, msg) { if (!cond) throw new Error(msg); }

const executablePath = process.env.CHROMIUM_PATH
  || ['/opt/pw-browsers/chromium-1194/chrome-linux/chrome', '/opt/pw-browsers/chromium/chrome-linux/chrome']
    .find(p => fs.existsSync(p));

let browser;
try {
  await waitForServer();
  browser = await chromium.launch(executablePath ? { executablePath } : {});

  // Real JPEG fixtures produced by the browser's own encoder
  const maker = await browser.newPage();
  const b64 = await maker.evaluate(() => {
    const c = document.createElement('canvas'); c.width = 64; c.height = 48;
    const g = c.getContext('2d'); g.fillStyle = '#1e8c3c'; g.fillRect(0, 0, 64, 48);
    return c.toDataURL('image/jpeg', 0.9).split(',')[1];
  });
  await maker.close();
  const jpeg = Buffer.from(b64, 'base64');
  for (const f of ['aurora_20260305_213100_00000.jpg', 'aurora_20260305_223100_00001_AURORA.jpg']) {
    fs.writeFileSync(path.join(capture, f), jpeg);
    fs.writeFileSync(path.join(capture, 'thumbs', f), jpeg);
  }
  const context = await browser.newContext({ viewport: { width: 390, height: 844 } }); // phone
  const page = await context.newPage();
  const jsErrors = [];
  page.on('pageerror', e => jsErrors.push(e.message));
  page.on('console', m => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) jsErrors.push(m.text()); });
  page.on('dialog', d => d.accept());

  const pages = ['index', 'dashboard', 'preview', 'settings', 'settings_advanced', 'presets', 'storage', 'gallery', 'diagnostics'];
  for (const p of pages) {
    await step(`page ${p}.html sans erreur JavaScript`, async () => {
      jsErrors.length = 0;
      const res = await page.goto(`${base}/${p}.html`);
      assert(res.ok(), 'HTTP ' + res.status());
      await page.waitForLoadState('networkidle');
      assert(jsErrors.length === 0, jsErrors.join(' | '));
    });
  }

  await step('dashboard : phase et avertissements', async () => {
    await page.goto(base + '/dashboard.html');
    await page.waitForFunction(() => document.getElementById('phaseBadge').textContent === 'ARM');
    const warnings = await page.locator('#warningsContainer .warning-box').allTextContents();
    assert(warnings.some(w => w.includes('Mot de passe Wi-Fi par défaut')), 'default password warning: ' + warnings);
  });

  await step('réglages rapides : sauvegarde et persistance', async () => {
    await page.goto(base + '/settings.html');
    await page.waitForLoadState('networkidle');
    await page.locator('#isoMax').evaluate(el => { el.value = '1600'; el.dispatchEvent(new Event('input')); });
    await page.evaluate(() => saveSettings());
    await page.waitForSelector('.toast.success');
    const saved = JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8'));
    assert(saved.exposure.iso_max === 1600, 'iso_max on disk = ' + saved.exposure.iso_max);
    assert(saved.network.password === 'aurora2024', 'password must be kept');
  });

  await step('réglages avancés : sauvegarde sans retaper le mot de passe (non-régression)', async () => {
    await page.goto(base + '/settings_advanced.html');
    await page.waitForLoadState('networkidle');
    assert(await page.inputValue('#wifiPassword') === '', 'password field must be empty');
    await page.evaluate(() => saveAdvanced());
    await page.waitForSelector('.toast.success');
  });

  await step('réglages avancés : mot de passe trop court refusé', async () => {
    await page.goto(base + '/settings_advanced.html');
    await page.waitForLoadState('networkidle');
    await page.fill('#wifiPassword', 'court');
    await page.evaluate(() => saveAdvanced());
    await page.waitForSelector('.toast.error');
  });

  await step('réglages avancés : nouveau mot de passe enregistré', async () => {
    await page.goto(base + '/settings_advanced.html');
    await page.waitForLoadState('networkidle');
    await page.fill('#wifiPassword', 'NouveauMotDePasse2026');
    await page.evaluate(() => saveAdvanced());
    await page.waitForSelector('.toast.success');
    const saved = JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8'));
    assert(saved.network.password === 'NouveauMotDePasse2026', 'password not saved');
  });

  await step('presets : créer, appliquer, supprimer', async () => {
    await page.goto(base + '/presets.html');
    await page.waitForSelector('.preset-card');
    await page.fill('#newPresetName', 'Test E2E');
    await page.evaluate(() => savePreset());
    await page.waitForSelector('.preset-name:text("Test E2E")');
    await page.locator('.preset-card', { hasText: 'Moonlight' }).locator('button[data-action=apply]').click();
    await page.waitForSelector('#presetStatus:has-text("appliqué")');
    const saved = JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8'));
    assert(saved.exposure.iso_max === 800, 'Moonlight preset not applied');
    assert(saved.network.password === 'NouveauMotDePasse2026', 'preset must not reset the password');
    await page.locator('.preset-card', { hasText: 'Test E2E' }).locator('button[data-action=delete]').click();
    await page.waitForFunction(() => ![...document.querySelectorAll('.preset-name')].some(e => e.textContent === 'Test E2E'));
  });

  await step('galerie : images, lightbox, session', async () => {
    await page.goto(base + '/gallery.html');
    await page.waitForSelector('.gallery-item');
    assert(await page.locator('.gallery-item').count() === 2, 'expected 2 images');
    await page.locator('.gallery-item').first().click();
    await page.waitForSelector('#lightbox.active');
    await page.waitForFunction(() => document.getElementById('lightboxImg').naturalWidth > 0);
    await page.evaluate(() => closeLightbox());
    await page.evaluate(() => switchTab ? switchTab('sessions') : null).catch(() => {});
  });

  await step('galerie : sessions listées sans suppression (non-régression)', async () => {
    const sessions = await (await fetch(base + '/api/gallery/sessions')).json();
    assert(sessions.length === 1 && sessions[0].image_count === 2, JSON.stringify(sessions));
    assert(fs.existsSync(path.join(capture, 'sessions/2026-03-05_21-30/event.jsonl')), 'session log deleted');
  });

  await step('galerie : suppression d\'une image', async () => {
    await page.goto(base + '/gallery.html');
    await page.waitForSelector('.gallery-item');
    await page.evaluate(() => { toggleSelect('aurora_20260305_213100_00000.jpg'); confirmDelete(); });
    await page.waitForSelector('#deleteModal.active');
    await page.evaluate(() => executeDelete());
    await page.waitForSelector('.toast.success');
    assert(!fs.existsSync(path.join(capture, 'aurora_20260305_213100_00000.jpg')), 'file still present');
    assert(!fs.existsSync(path.join(capture, 'thumbs/aurora_20260305_213100_00000.jpg')), 'thumbnail still present');
  });

  await step('diagnostics : version affichée', async () => {
    await page.goto(base + '/diagnostics.html');
    await page.waitForFunction(() => document.getElementById('sysVersion').textContent.startsWith('v'));
  });

  await step('portail captif : redirection vers l\'interface', async () => {
    const res = await fetch(base + '/generate_204', { redirect: 'manual' });
    assert(res.status === 307 && res.headers.get('location').startsWith('http://192.168.4.1:'), 'status ' + res.status);
  });

  await step('lancement de la nuit depuis le dashboard', async () => {
    await page.goto(base + '/dashboard.html');
    await page.waitForFunction(() => document.getElementById('phaseBadge').textContent === 'ARM');
    await page.click('#disconnectBtn');
    await page.waitForFunction(() => document.getElementById('phaseBadge').textContent === 'DISCONNECT', null, { timeout: 8000 });
    assert(await page.locator('#disconnectBtn').isDisabled(), 'button must be disabled during the night');
  });
} finally {
  if (browser) await browser.close();
  server.kill();
  fs.rmSync(work, { recursive: true, force: true });
}

console.log(failures === 0 ? '\nInterface web : tous les tests OK' : `\nInterface web : ${failures} échec(s)`);
process.exit(failures === 0 ? 0 : 1);
