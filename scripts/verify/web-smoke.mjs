// web-smoke.mjs — headless click-through of the Tauri frontend (rust/web/).
//
// Implements the "GUI frontend verification (browser click-through)" recipe
// in CLAUDE.md. Run by scripts/verify.sh (layer "web"); usable on its own:
//
//   NODE_PATH=<dir>/node_modules node scripts/verify/web-smoke.mjs \
//     --web-dir rust/web --config rust/config.example.json \
//     --out-dir rust/target/verify --chrome <path/to/chrome> --version 2.4.0
//
// The page cannot load without window.__TAURI__ (ipc.js throws), so a mock
// is injected via addInitScript before any module script runs. The mock keeps
// the config in memory, applies the mutating commands the UI uses and emits
// `config-changed` after each one (iron rule 10 — the real backend does too).
//
// Exit code 0 = every check passed; 1 = at least one check failed.

import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core');

// ─── Args ────────────────────────────────────────────────────────────────────

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    const k = argv[i];
    if (!k.startsWith('--')) throw new Error(`unexpected argument ${k}`);
    out[k.slice(2)] = argv[i + 1];
  }
  for (const req of ['web-dir', 'config', 'out-dir', 'chrome']) {
    if (!out[req]) throw new Error(`missing --${req}`);
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
const webDir = path.resolve(args['web-dir']);
const outDir = path.resolve(args['out-dir']);
const exampleConfig = JSON.parse(fs.readFileSync(args.config, 'utf8'));
const appVersion = args.version || '0.0.0-mock';
fs.mkdirSync(outDir, { recursive: true });

// ─── Result bookkeeping ──────────────────────────────────────────────────────

const checks = [];   // { name, ok, detail }
function check(name, ok, detail = '') {
  checks.push({ name, ok: !!ok, detail });
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ' — ' + detail : ''}`);
}

// ─── Static server ───────────────────────────────────────────────────────────

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
};

const httpMisses = [];   // paths that 404'd
function startServer() {
  const server = http.createServer((req, res) => {
    const urlPath = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    const rel = urlPath === '/' ? 'index.html' : urlPath.replace(/^\/+/, '');
    const file = path.resolve(webDir, rel);
    if (!file.startsWith(webDir + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
      httpMisses.push(urlPath);
      res.writeHead(404);
      res.end('not found');
      return;
    }
    res.writeHead(200, { 'Content-Type': MIME[path.extname(file)] || 'application/octet-stream' });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise(resolve => {
    server.listen(0, '127.0.0.1', () => resolve(server));
  });
}

// ─── Tauri mock (runs inside the page, before module scripts) ────────────────

function tauriMock({ config, version }) {
  const listeners = {};
  let nextId = 1;
  const state = {
    config: JSON.parse(JSON.stringify(config)),
    prefs: { drawer_open: false, last_tab: null },
  };
  window.__mockCalls = [];
  window.__mockCallArgs = [];
  window.__mockErrors = [];
  window.addEventListener('error', e => {
    window.__mockErrors.push(`error: ${e.message}`);
  });
  window.addEventListener('unhandledrejection', e => {
    window.__mockErrors.push(`unhandledrejection: ${e.reason && (e.reason.stack || e.reason)}`);
  });

  function emit(name, payload) {
    for (const l of (listeners[name] || []).slice()) {
      l.cb({ event: name, id: l.id, payload });
    }
  }
  function emitConfigChanged() {
    setTimeout(() => emit('config-changed', { source: 'ipc' }), 0);
  }
  window.__mockEmit = emit;

  // Shapes mirror rust/src/gui/commands.rs + rust/src/engine.rs (per-pad
  // config): get_controller_status → ControllerStatus { name, transport,
  // kind, warnings, connected }; get_config → { version: 2, pads: {...} }.
  const status = {
    name: 'DualSense Wireless Controller',
    transport: 'BT',
    kind: 'dualsense',
    warnings: [],
    connected: ['dualsense'],
  };
  const PAGE_FIELD = { base: 'buttons', quick_l: 'quick_l', quick_r: 'quick_r' };

  async function invoke(cmd, a) {
    window.__mockCalls.push(cmd);
    window.__mockCallArgs.push({ cmd, args: a === undefined ? null : JSON.parse(JSON.stringify(a)) });
    const clone = v => JSON.parse(JSON.stringify(v));
    switch (cmd) {
      case 'get_controller_status': return clone(status);
      case 'get_config':            return clone(state.config);
      case 'get_ui_prefs':          return clone(state.prefs);
      case 'set_ui_prefs':          state.prefs = clone(a.prefs); return null;
      case 'get_app_version':       return version;
      case 'set_binding': {
        // Like the backend: a missing pad / page is an IPC error.
        const pad = state.config.pads?.[a.pad];
        const field = PAGE_FIELD[a.page];
        if (!pad || !field) throw new Error(`set_binding: bad pad/page ${a.pad}/${a.page}`);
        pad[field] = pad[field] || {};
        if (a.page !== 'base' && a.entry.type === 'unbound') delete pad[field][String(a.id)];
        else pad[field][String(a.id)] = clone(a.entry);
        emitConfigChanged();
        return null;
      }
      case 'set_macro':
        state.config.macros = state.config.macros || {};
        state.config.macros[a.name] = clone(a.def);
        emitConfigChanged();
        return null;
      case 'set_settings':
        Object.assign(state.config, clone(a.settings));
        emitConfigChanged();
        return null;
      case 'reset_settings':
        emitConfigChanged();
        return null;
      default:
        return null;
    }
  }

  window.__TAURI__ = {
    core: { invoke },
    event: {
      listen: async (name, cb) => {
        const id = nextId++;
        (listeners[name] = listeners[name] || []).push({ id, cb });
        return () => {
          listeners[name] = (listeners[name] || []).filter(l => l.id !== id);
        };
      },
    },
  };
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async function clickEveryButtonIn(page, paneSel, maxRounds = 40) {
  // Buttons re-render after most clicks, so re-query each round and click
  // each distinct label once. Disabled buttons are recorded, not clicked.
  const clicked = [];
  const skipped = new Set();
  for (let round = 0; round < maxRounds; round++) {
    const buttons = page.locator(`${paneSel} button`);
    const n = await buttons.count();
    let target = null;
    for (let i = 0; i < n; i++) {
      const b = buttons.nth(i);
      const label = ((await b.textContent()) || '').trim() || `(button #${i})`;
      if (clicked.includes(label) || skipped.has(label)) continue;
      if (!(await b.isVisible())) continue;
      if (await b.isDisabled()) { skipped.add(label); continue; }
      target = { b, label };
      break;
    }
    if (!target) break;
    await target.b.click({ timeout: 3000 });
    clicked.push(target.label);
    await page.waitForTimeout(50);
  }
  return { clicked, skipped: [...skipped] };
}

async function popupOpen(page) {
  return page.locator('.bind-popup-overlay').isVisible();
}

async function closePopup(page) {
  await page.locator('.bind-popup-overlay .bp-cancel').click({ timeout: 3000 });
  await page.locator('.bind-popup-overlay').waitFor({ state: 'detached', timeout: 3000 });
}

// ─── Main ────────────────────────────────────────────────────────────────────

const server = await startServer();
const base = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch({ executablePath: args.chrome, headless: true });
const consoleErrors = [];
const pageErrors = [];
const dialogs = [];

try {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  await context.addInitScript(tauriMock, { config: exampleConfig, version: appVersion });
  const page = await context.newPage();
  page.on('pageerror', e => pageErrors.push(String(e && (e.stack || e))));
  page.on('console', m => {
    if (m.type() !== 'error') return;
    const url = (m.location() && m.location().url) || '';
    // Chromium logs every 404 as a console error; the favicon miss is the
    // one expected request (CLAUDE.md), and is accounted for via httpMisses.
    if (/Failed to load resource/.test(m.text()) && /favicon\.ico/.test(url)) return;
    consoleErrors.push(`${m.text()} @ ${url}`);
  });
  page.on('dialog', async d => { dialogs.push(`${d.type()}: ${d.message()}`); await d.dismiss(); });

  await page.goto(base, { waitUntil: 'load' });
  await page.locator('#controller-host svg.controller').waitFor({ timeout: 10000 });
  await page.waitForFunction(() => document.querySelectorAll('#chip-list .chip-row').length > 0,
    null, { timeout: 10000 });

  // Status bar — seeded from get_controller_status (v2.4.0 shape).
  const statusSeed = await page.locator('#status-text').textContent();
  check('status seeded from get_controller_status', /Connected/.test(statusSeed) && /BT/.test(statusSeed), statusSeed);

  // controller-status event, v2.4.0 shape (kind + transport).
  await page.evaluate(() => window.__mockEmit('controller-status',
    { connected: false, name: '', transport: '', kind: null }));
  const statusOff = await page.locator('#status-text').textContent();
  check('controller-status disconnected event', /Waiting/.test(statusOff), statusOff);
  await page.evaluate(() => window.__mockEmit('controller-status',
    { connected: true, name: 'DualSense Wireless Controller', transport: 'BT', kind: 'dualsense' }));
  const statusOn = await page.locator('#status-text').textContent();
  check('controller-status connected event (kind, transport BT)', /Connected/.test(statusOn) && /BT/.test(statusOn), statusOn);

  const chipRows = await page.locator('#chip-list .chip-row').count();
  check('chip list renders 33 rows (ids 0..32)', chipRows === 33, `${chipRows} rows`);
  // v2.5.0: id 29 (Capture, Switch Pro) must be listed and bindable.
  const capRow = page.locator('#chip-list .chip-row[data-id="29"]');
  const capCount = await capRow.count();
  let capOpens = false;
  if (capCount === 1) {
    await capRow.click();
    capOpens = await popupOpen(page);
    if (capOpens) await closePopup(page);
  }
  check('chip row for id 29 (Capture) exists and opens .bind-popup-overlay', capCount === 1 && capOpens,
    `rows=${capCount} popup=${capOpens}`);
  // v2.6.0: id 32 (C, Switch 2 Pro) must be listed and bindable.
  const cRow = page.locator('#chip-list .chip-row[data-id="32"]');
  const cCount = await cRow.count();
  let cOpens = false;
  if (cCount === 1) {
    await cRow.click();
    cOpens = await popupOpen(page);
    if (cOpens) await closePopup(page);
  }
  check('chip row for id 32 (C) exists and opens .bind-popup-overlay', cCount === 1 && cOpens,
    `rows=${cCount} popup=${cOpens}`);
  await page.screenshot({ path: path.join(outDir, 'web-main.png') });

  // Engine events the frontend listens to — must not throw.
  await page.evaluate(() => {
    const e = window.__mockEmit;
    e('button-down', { id: 0 });
    e('button-up', { id: 0 });
    e('touchpad-click', { raw_x: 500, raw_y: 300, quadrant: 0 });
    e('touchpad-hover', { raw_x: 1500, raw_y: 700, quadrant: 3 });
    e('touchpad-hover', { raw_x: 0, raw_y: 0, quadrant: 255 });
    e('activity', { ts_ms: Date.now(), kind: 'emit', key: 'x', action: 'down' });
    e('activity', { ts_ms: Date.now(), kind: 'macro-start', name: 'macro_A' });
    e('activity', { ts_ms: Date.now(), kind: 'macro-end', name: 'macro_A', completed: true });
  });

  // Toolbar: activity drawer.
  await page.click('#btn-activity');
  const drawerOpen = await page.locator('#activity-drawer.open').count();
  await page.evaluate(() => window.__mockEmit('activity', { ts_ms: Date.now(), kind: 'emit', key: 'z', action: 'up' }));
  await page.waitForTimeout(100);
  const rows = await page.locator('#activity-list .activity-row').count();
  check('#btn-activity opens drawer and renders rows', drawerOpen === 1 && rows > 0, `open=${drawerOpen} rows=${rows}`);
  await page.click('#btn-activity-close');
  check('#btn-activity-close closes drawer', (await page.locator('#activity-drawer.open').count()) === 0);

  // Toolbar: settings shortcut.
  await page.click('#btn-settings');
  check('#btn-settings activates settings pane', (await page.locator('#pane-settings.active').count()) === 1);

  // Every tab.
  const tabNames = await page.locator('[data-tab]').evaluateAll(els => els.map(e => e.dataset.tab));
  for (const t of tabNames) {
    await page.click(`[data-tab="${t}"]`);
    const active = await page.locator(`#pane-${t}.active`).count();
    check(`tab ${t} activates #pane-${t}`, active === 1);
  }

  // Macros pane: select the first macro so the editor (and its buttons) render.
  await page.click('[data-tab="macros"]');
  const macroRows = page.locator('#pane-macros .macro-row');
  if ((await macroRows.count()) > 0) await macroRows.first().click();
  const macroRes = await clickEveryButtonIn(page, '#pane-macros');
  check('every #pane-macros button clicks', macroRes.clicked.length >= 4,
    `clicked [${macroRes.clicked.join(', ')}]${macroRes.skipped.length ? ` disabled [${macroRes.skipped.join(', ')}]` : ''}`);

  // Settings pane.
  await page.click('[data-tab="settings"]');
  const settingsRes = await clickEveryButtonIn(page, '#pane-settings');
  const settingsErr = await page.locator('.settings-error').isVisible().catch(() => false);
  check('every #pane-settings button clicks', settingsRes.clicked.length >= 3 && !settingsErr,
    `clicked [${settingsRes.clicked.join(', ')}] settings-error visible=${settingsErr}`);

  // Mappings: every chip row opens the bind popup.
  await page.click('[data-tab="mappings"]');
  const nRows = await page.locator('#chip-list .chip-row').count();
  let opened = 0;
  for (let i = 0; i < nRows; i++) {
    await page.locator('#chip-list .chip-row').nth(i).click();
    if (await popupOpen(page)) opened++;
    if (i === 0) {
      for (const seg of ['key', 'macro', 'mouse', 'unbound']) {
        await page.click(`.bind-popup-overlay .seg-item[data-seg="${seg}"]`);
      }
      await page.click('.bind-popup-overlay .seg-item[data-seg="key"]');
      await page.screenshot({ path: path.join(outDir, 'web-bind-popup.png') });
    }
    if (await popupOpen(page)) await closePopup(page);
  }
  check('every #chip-list .chip-row opens .bind-popup-overlay', opened === nRows, `${opened}/${nRows}`);

  // Save path: rebind row 0 to Unbound → set_binding → config-changed → chip list refresh.
  await page.locator('#chip-list .chip-row').first().click();
  await page.click('.bind-popup-overlay .seg-item[data-seg="unbound"]');
  await page.click('.bind-popup-overlay .bp-save');
  await page.locator('.bind-popup-overlay').waitFor({ state: 'detached', timeout: 3000 });
  await page.waitForTimeout(100);
  const row0Unbound = await page.locator('#chip-list .chip-row').first().locator('.chip-mute').count();
  check('bind popup Save → set_binding → chip list refreshed', row0Unbound === 1);

  // Key capture path: row 0 → Key → press F → Save → set_binding aimed at
  // the shown pad's base table.
  await page.locator('#chip-list .chip-row').first().click();
  await page.click('.bind-popup-overlay .seg-item[data-seg="key"]');
  await page.waitForTimeout(50);
  await page.keyboard.press('f');
  await page.click('.bind-popup-overlay .bp-save');
  await page.locator('.bind-popup-overlay').waitFor({ state: 'detached', timeout: 3000 });
  await page.waitForTimeout(100);
  const lastBind = await page.evaluate(() =>
    window.__mockCallArgs.filter(c => c.cmd === 'set_binding').pop());
  check('Key capture Save → set_binding { pad: dualsense, page: base, id 0, key F }',
    lastBind?.args?.pad === 'dualsense' && lastBind?.args?.page === 'base' &&
      lastBind?.args?.id === 0 && lastBind?.args?.entry?.type === 'key' &&
      /^f$/i.test(lastBind?.args?.entry?.value ?? ''),
    JSON.stringify(lastBind));
  const row0Key = await page.locator('#chip-list .chip-row').first().textContent();
  check('chip list shows the new key after config-changed', /\bF\b/i.test(row0Key), row0Key);

  // The shown table follows the connected pad: the Switch Pro's touchpad
  // id 25 is unbound, the DualSense's is mouse left.
  const row25 = () => page.locator('#chip-list .chip-row[data-id="25"] .chip-mute').count();
  const dsRow25Unbound = await row25();
  await page.evaluate(() => window.__mockEmit('controller-status',
    { connected: true, name: 'Switch Pro Controller', transport: 'BT', kind: 'ns_pro', connected_pads: ['ns_pro'] }));
  await page.waitForTimeout(150);
  const nsRow25Unbound = await row25();
  await page.evaluate(() => window.__mockEmit('controller-status',
    { connected: true, name: 'DualSense Wireless Controller', transport: 'BT', kind: 'dualsense', connected_pads: ['dualsense'] }));
  await page.waitForTimeout(150);
  check('chip list follows the connected pad (ns_pro table, then back to dualsense)',
    dsRow25Unbound === 0 && nsRow25Unbound === 1 && (await row25()) === 0,
    `dualsense=${dsRow25Unbound} ns_pro=${nsRow25Unbound}`);

  // Controller hotspots: click each [data-id] hit zone at its centre.
  const hotspotIds = await page.locator('#controller-host svg.controller [data-id]')
    .evaluateAll(els => els.map(e => e.dataset.id));
  let hsOpened = 0;
  const hsMismatch = [];
  for (const id of hotspotIds) {
    const box = await page.locator(`#controller-host svg.controller [data-id="${id}"]`).first().boundingBox();
    if (!box) continue;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    if (await popupOpen(page)) {
      hsOpened++;
      const sub = await page.locator('.bind-popup-overlay .bp-sub').textContent();
      if (sub.trim() !== `id ${id}`) hsMismatch.push(`${id}→${sub.trim().replace('id ', '')}`);
      await closePopup(page);
    }
  }
  check('controller hotspots open .bind-popup-overlay', hotspotIds.length > 0 && hsOpened === hotspotIds.length,
    `${hsOpened}/${hotspotIds.length}${hsMismatch.length ? `; centre-click landed on another zone: ${hsMismatch.join(' ')}` : ''}`);

  // Disconnect button.
  await page.click('#btn-disconnect');
  await page.waitForTimeout(50);
  const calls = await page.evaluate(() => window.__mockCalls);
  check('#btn-disconnect invokes disconnect_gamepad', calls.includes('disconnect_gamepad'));

  // Layout: no horizontal overflow; window must not rubber-band.
  for (const width of [800, 1024, 1280, 1600]) {
    await page.setViewportSize({ width, height: 800 });
    await page.waitForTimeout(100);
    const m = await page.evaluate(() => ({
      sw: document.documentElement.scrollWidth,
      cw: document.documentElement.clientWidth,
    }));
    check(`no horizontal overflow @${width}px`, m.sw === m.cw, `scrollWidth=${m.sw} clientWidth=${m.cw}`);
  }
  const ob = await page.evaluate(() => ({
    html: getComputedStyle(document.documentElement).overscrollBehaviorX,
    body: getComputedStyle(document.body).overscrollBehaviorX,
  }));
  check('overscroll-behavior-x none on html/body', ob.html === 'none' && ob.body === 'none', JSON.stringify(ob));

  const inPageErrors = await page.evaluate(() => window.__mockErrors);
  const otherMisses = httpMisses.filter(p => p !== '/favicon.ico');
  check('zero pageerror', pageErrors.length === 0, pageErrors.join(' | '));
  check('zero console errors', consoleErrors.length === 0, consoleErrors.join(' | '));
  check('zero window error / unhandledrejection', inPageErrors.length === 0, inPageErrors.join(' | '));
  check('no unexpected dialogs (alert/confirm/prompt)', dialogs.length === 0, dialogs.join(' | '));
  check('no 404 except favicon', otherMisses.length === 0, otherMisses.join(' '));
  console.log(`info: IPC calls seen: ${[...new Set(calls)].sort().join(', ')}`);
  console.log(`info: screenshots: ${path.join(outDir, 'web-main.png')} ${path.join(outDir, 'web-bind-popup.png')}`);
} catch (e) {
  check('click-through ran to completion', false, String(e && (e.stack || e)));
} finally {
  await browser.close();
  server.close();
}

const failed = checks.filter(c => !c.ok).length;
console.log(`web-smoke: ${checks.length - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
