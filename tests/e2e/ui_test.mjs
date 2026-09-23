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
const ev = (n, score) => JSON.stringify({
  timestamp: `2026-03-05T2${n + 1}:31:00`, phase: 'Run', capture_mode: 'SAFE', exposure_us: 5000000, iso: 800,
  format: 'Jpg', roi_excluded_percent: 35, aurora_score: score, aurora_detected: score > 1, aurora_color: 'green',
  consecutive_hits: 0, moon_mask_active: false, frame_number: n,
});
fs.writeFileSync(path.join(capture, 'sessions/2026-03-05_21-30/event.jsonl'), ev(0, 1.1) + '\n' + ev(1, 4.2) + '\n');

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
  // Layout 1.7: one folder per night (JPG, RAW, thumbs)
  const nightDir = path.join(capture, 'sessions/2026-03-05_21-30');
  for (const d of ['JPG', 'RAW', 'thumbs']) fs.mkdirSync(path.join(nightDir, d), { recursive: true });
  for (const f of ['aurora_20260305_213100_00000.jpg', 'aurora_20260305_223100_00001_AURORA.jpg']) {
    fs.writeFileSync(path.join(nightDir, 'JPG', f), jpeg);
    fs.writeFileSync(path.join(nightDir, 'thumbs', f), jpeg);
  }
  fs.writeFileSync(path.join(nightDir, 'RAW', 'aurora_20260305_223100_00001_AURORA.dng'), Buffer.from('DNG'));
  const context = await browser.newContext({ viewport: { width: 390, height: 844 } }); // phone
  const page = await context.newPage();
  const jsErrors = [];
  page.on('pageerror', e => jsErrors.push(e.message));
  page.on('console', m => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) jsErrors.push(m.text()); });
  page.on('dialog', d => d.accept());

  const pages = ['index', 'preview', 'settings', 'settings_advanced', 'presets', 'storage', 'gallery', 'diagnostics'];
  for (const p of pages) {
    await step(`page ${p}.html sans erreur JavaScript`, async () => {
      jsErrors.length = 0;
      const res = await page.goto(`${base}/${p}.html`);
      assert(res.ok(), 'HTTP ' + res.status());
      await page.waitForLoadState('networkidle');
      assert(jsErrors.length === 0, jsErrors.join(' | '));
    });
  }

  await step('accueil : vérifications avant la nuit', async () => {
    await page.goto(base + '/index.html');
    await page.waitForFunction(() => document.getElementById('phaseBadge').textContent === 'ARM');
    await page.waitForSelector('#checks [data-check=usb]');
    assert(await page.locator('#checks [data-check=camera].ok').count() === 1, 'camera ok');
    assert(await page.locator('#checks [data-check=password].warn').count() === 1, 'default password warning');
    assert(!(await page.locator('#startNightBtn').isDisabled()), 'night can be started');
    assert((await page.textContent('#capacityLine')).includes('Place sur la clé'), 'capacity shown');
  });

  await step('accueil : résumé de la dernière nuit et lien RAW', async () => {
    await page.goto(base + '/index.html');
    await page.waitForSelector('#lastNight:not([hidden])');
    const href = await page.getAttribute('#lastAll', 'href');
    assert(href === '/api/gallery/sessions/2026-03-05_21-30/download', href);
    assert((await page.getAttribute('#lastRaw', 'href')).endsWith('?only=raw'), 'raw link');
  });

  await step('accueil : menu simple puis mode expert', async () => {
    await page.goto(base + '/index.html');
    await page.evaluate(() => { try { localStorage.removeItem('aurionExpert'); } catch (_) { } });
    await page.reload();
    await page.click('.burger-btn');
    assert(await page.locator('#sideMenu a[href="/presets.html"]').count() === 0, 'presets hidden in simple mode');
    await page.check('#expertToggle');
    assert(await page.locator('#sideMenu a[href="/presets.html"]').count() === 1, 'presets shown in expert mode');
    await page.goto(base + '/gallery.html');
    assert(await page.locator('#sideMenu a[href="/presets.html"]').count() === 1, 'expert mode remembered');
  });

  await step('ancien dashboard redirigé vers l\'accueil', async () => {
    await page.goto(base + '/dashboard.html');
    await page.waitForURL(/index\.html$/);
  });

  await step('accueil : mot de passe personnel au premier démarrage', async () => {
    await page.goto(base + '/index.html');
    await page.waitForSelector('#onboarding:not([hidden])');
    await page.fill('#newWifiPassword', 'court');
    await page.click('#onboarding .btn-primary');
    await page.waitForSelector('.toast.error');
    assert(JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8')).network.password === 'aurora2024', 'short password refused');
    await page.click('#onboarding .btn-outline');
    assert(await page.locator('#onboarding').isHidden(), 'later hides the banner');
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

  await step('réglages photo : balance des blancs, verrou d\'exposition, empilement', async () => {
    await page.goto(base + '/settings_advanced.html');
    await page.waitForLoadState('networkidle');
    assert(await page.inputValue('#awbMode') === 'daylight', 'daylight by default');
    await page.selectOption('#awbMode', 'cloudy');
    await page.check('#lockInRun');
    await page.fill('#stackFrames', '4');
    await page.evaluate(() => saveAdvanced());
    await page.waitForSelector('.toast.success');
    const saved = JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8'));
    assert(saved.capture.awb === 'cloudy' && saved.exposure.lock_in_run === true && saved.capture.denoise.stack_frames === 4,
      JSON.stringify([saved.capture.awb, saved.exposure.lock_in_run, saved.capture.denoise]));
  });

  await step('preview : darks refusés sans preview préalable', async () => {
    await page.goto(base + '/preview.html');
    await page.waitForLoadState('networkidle');
    await page.evaluate(() => captureDarks());
    await page.waitForSelector('.toast.error');
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
    assert(await page.locator('.gallery-item').count() === 3, 'expected 2 JPG + 1 RAW');
    await page.locator('.gallery-item').first().click();
    await page.waitForSelector('#lightbox.active');
    await page.waitForFunction(() => document.getElementById('lightboxImg').naturalWidth > 0);
    await page.evaluate(() => closeLightbox());
    await page.evaluate(() => switchTab ? switchTab('sessions') : null).catch(() => {});
  });

  await step('galerie : onglet meilleures aurores', async () => {
    await page.goto(base + '/gallery.html');
    await page.waitForSelector('.gallery-item');
    await page.click('#tabBest');
    await page.waitForSelector('#bestContent .gallery-item[data-best]');
    const first = await page.locator('#bestContent .item-info').first().textContent();
    assert(first.includes('score 4.20'), 'strongest first: ' + first);
  });

  await step('galerie : téléchargement RAW / JPG par nuit', async () => {
    await page.goto(base + '/gallery.html#sessions');
    await page.waitForSelector('#paneSessions a[data-dl=raw]');
    const href = await page.getAttribute('#paneSessions a[data-dl=jpg]', 'href');
    const res = await fetch(base + href);
    assert(res.ok && res.headers.get('content-disposition').includes('_JPG.zip'), 'jpg zip ' + res.status);
    await res.arrayBuffer();
  });

  await step('galerie : sessions listées sans suppression (non-régression)', async () => {
    const sessions = await (await fetch(base + '/api/gallery/sessions')).json();
    assert(sessions.length === 1 && sessions[0].image_count === 3 && sessions[0].raw_count === 1, JSON.stringify(sessions));
    assert(fs.existsSync(path.join(capture, 'sessions/2026-03-05_21-30/event.jsonl')), 'session log deleted');
  });

  await step('galerie : suppression d\'une image', async () => {
    await page.goto(base + '/gallery.html');
    await page.waitForSelector('.gallery-item');
    await page.evaluate(() => { toggleSelect('aurora_20260305_213100_00000.jpg'); confirmDelete(); });
    await page.waitForSelector('#deleteModal.active');
    await page.evaluate(() => executeDelete());
    await page.waitForSelector('.toast.success');
    assert(!fs.existsSync(path.join(nightDir, 'JPG/aurora_20260305_213100_00000.jpg')), 'file still present');
    assert(!fs.existsSync(path.join(nightDir, 'thumbs/aurora_20260305_213100_00000.jpg')), 'thumbnail still present');
  });

  await step('stockage : bouton préparer la clé (mode expert)', async () => {
    await page.goto(base + '/storage.html');
    await page.waitForSelector('#prepareKeyBtn');
    assert((await page.textContent('#prepareKeyBtn')).includes('efface'), 'the button says it erases');
  });

  await step('diagnostics : version affichée et mise à jour depuis GitHub', async () => {
    await page.goto(base + '/diagnostics.html');
    await page.waitForFunction(() => /^\d+\.\d+\.\d+/.test(document.getElementById('sysVersion').textContent));
    await page.waitForFunction(() => /^\d+\.\d+\.\d+/.test(document.getElementById('currentVersion').textContent));
    assert(await page.locator('#updChannel option[value=dev]').count() === 1, 'dev channel offered');
    assert(await page.locator('#rollbackBtn').isHidden(), 'no previous version yet');
  });

  await step('portail captif : redirection vers l\'interface', async () => {
    const res = await fetch(base + '/generate_204', { redirect: 'manual' });
    assert(res.status === 307 && res.headers.get('location').startsWith('http://192.168.4.1:'), 'status ' + res.status);
  });

  await step('accueil : mode expédition et RAW par défaut', async () => {
    await page.goto(base + '/index.html');
    await page.waitForSelector('#checks [data-check=usb]');
    await page.check('#expeditionToggle');
    await page.waitForSelector('#autoStartCard:not([hidden])');
    await page.waitForSelector('#checks [data-check=expedition]');
    assert((await page.textContent('#autoStartText')).includes('5 minutes'), 'auto-start explained');
    let saved = JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8'));
    assert(saved.expedition.enabled === true, 'expedition saved');
    await page.uncheck('#expeditionToggle');
    await page.waitForSelector('#autoStartCard[hidden]', { state: 'attached' });
    saved = JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8'));
    assert(saved.expedition.enabled === false, 'expedition off');
    assert(await page.inputValue('#nightFormat') === 'RawDng', 'RAW only by default');
    assert((await page.locator('#nightFormat option').first().textContent()).includes('conseillé'), 'RAW recommended first');
  });

  await step('lancement de la nuit depuis l\'accueil (mode, durée, format)', async () => {
    await page.goto(base + '/index.html');
    await page.waitForFunction(() => !document.getElementById('startNightBtn').disabled);
    await page.check('#modeFilter');
    await page.selectOption('#nightLength', '6');
    await page.selectOption('#nightFormat', 'RawDng');
    await page.click('#startNightBtn');
    await page.waitForSelector('#nightRunning:not([hidden])', { timeout: 8000 });
    const saved = JSON.parse(fs.readFileSync(path.join(configDir, 'aurion.json'), 'utf8'));
    assert(saved.detection.detection_capture_enabled === true && saved.time_range.duration_hours === 6
      && saved.capture.output_format === 'RawDng', JSON.stringify([saved.detection.detection_capture_enabled, saved.time_range, saved.capture.output_format]));
    assert(saved.network.password === 'NouveauMotDePasse2026', 'password kept');
    const st = await (await fetch(base + '/api/status')).json();
    assert(st.phase !== 'ARM', 'phase ' + st.phase);
  });
} finally {
  if (browser) await browser.close();
  server.kill();
  fs.rmSync(work, { recursive: true, force: true });
}

console.log(failures === 0 ? '\nInterface web : tous les tests OK' : `\nInterface web : ${failures} échec(s)`);
process.exit(failures === 0 ? 0 : 1);
