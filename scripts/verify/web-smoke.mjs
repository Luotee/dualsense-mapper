// web-smoke.mjs — headless click-through of the key-binding board (rust/web/).
//
// Implements the "GUI frontend verification (browser click-through)" recipe
// in CLAUDE.md. Run by scripts/verify.sh (layer "web"); usable on its own:
//
//   NODE_PATH=<dir>/node_modules node scripts/verify/web-smoke.mjs \
//     --web-dir rust/web --config rust/config.example.json \
//     --out-dir rust/target/verify --chrome <path/to/chrome> --version 3.0.0
//
// The page cannot load without window.__TAURI__ (ipc.js throws), so a mock
// is injected via addInitScript before any module script runs. The mock keeps
// the config in memory, applies every mutating command the board uses, emits
// `config-changed` after each one (iron rule 10 — the real backend does too)
// and records every call with its arguments.
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
// Serve with the app's own Content-Security-Policy, so anything WebView2
// would block (inline script, a foreign font, …) fails here as a console error.
const tauriConf = JSON.parse(fs.readFileSync(path.join(webDir, '..', 'tauri.conf.json'), 'utf8'));
const csp = tauriConf.app?.security?.csp || '';
fs.mkdirSync(outDir, { recursive: true });

// ─── Result bookkeeping ──────────────────────────────────────────────────────

const checks = [];   // { name, ok, detail }
function check(name, ok, detail = '') {
  checks.push({ name, ok: !!ok, detail });
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ' — ' + detail : ''}`);
}

// ─── Static server ───────────────────────────────────────────────────────────

const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png',
  '.ico': 'image/x-icon', '.woff2': 'font/woff2',
};

const httpMisses = [];
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
    res.writeHead(200, { 'Content-Type': MIME[path.extname(file)] || 'application/octet-stream', ...(csp ? { 'Content-Security-Policy': csp } : {}) });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise(resolve => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// ─── Tauri mock (runs inside the page, before module scripts) ────────────────

function tauriMock({ config, version }) {
  const listeners = {};
  let nextId = 1;
  const clone = v => JSON.parse(JSON.stringify(v));
  const defaults = clone(config.pads);   // the example's pads = the default tables
  const state = {
    config: clone(config),
    prefs: { drawer_open: false, last_tab: null, last_pad: null },
    status: { name: 'DualSense', transport: 'BT', kind: 'dualsense', warnings: [], connected: ['dualsense'] },
    sticks: null,
  };
  window.__mock = state;
  window.__mockCalls = [];
  window.__mockErrors = [];
  window.__mockFail = {};   // cmd -> error string: the next call of cmd rejects
  window.addEventListener('error', e => window.__mockErrors.push(`error: ${e.message}`));
  window.addEventListener('unhandledrejection', e =>
    window.__mockErrors.push(`unhandledrejection: ${e.reason && (e.reason.stack || e.reason)}`));

  function emit(name, payload) {
    for (const l of (listeners[name] || []).slice()) l.cb({ event: name, id: l.id, payload });
  }
  const changed = () => setTimeout(() => emit('config-changed', { source: 'ipc' }), 0);
  window.__mockEmit = emit;
  const FIELD = { base: 'buttons', quick_l: 'quick_l', quick_r: 'quick_r' };
  const LABELS = {};
  for (const [id, e] of Object.entries(config.pads.dualsense.buttons)) LABELS[id] = e.label;

  function put(pad, page, id, entry) {
    const t = state.config.pads[pad][FIELD[page]];
    if (page !== 'base' && entry.type === 'unbound') delete t[String(id)];
    else t[String(id)] = clone(entry);
  }
  function get(pad, page, id) {
    return state.config.pads[pad][FIELD[page]][String(id)] || { label: LABELS[id] || '', type: 'unbound' };
  }
  function renameRefs(oldName, newName) {
    for (const p of Object.values(state.config.pads)) for (const f of ['buttons', 'quick_l', 'quick_r']) {
      for (const [id, e] of Object.entries(p[f] || {})) {
        if (e.type === 'macro' && e.value === oldName) {
          if (newName) e.value = newName; else if (f === 'buttons') p[f][id] = { label: e.label, type: 'unbound' }; else delete p[f][id];
        }
      }
    }
  }

  async function invoke(cmd, a) {
    window.__mockCalls.push({ cmd, args: a === undefined ? null : clone(a) });
    if (window.__mockFail[cmd]) { const m = window.__mockFail[cmd]; delete window.__mockFail[cmd]; throw m; }
    const c = state.config;
    switch (cmd) {
      case 'get_controller_status': return clone(state.status);
      case 'get_config': return clone(c);
      case 'get_ui_prefs': return clone(state.prefs);
      case 'set_ui_prefs': state.prefs = clone(a.prefs); return null;
      case 'get_app_version': return version;
      case 'get_config_path': return 'D:\\Games\\dualsense-mapper\\dualsense-mapper.json';
      case 'get_stick_positions': return state.sticks ? clone(state.sticks) : null;
      case 'set_binding': put(a.pad, a.page, a.id, a.entry); changed(); return null;
      case 'swap_bindings': {
        const ea = get(a.pad, a.pageA, a.idA), eb = get(a.pad, a.pageB, a.idB);
        put(a.pad, a.pageA, a.idA, { ...eb, label: ea.label });
        put(a.pad, a.pageB, a.idB, { ...ea, label: eb.label });
        changed(); return null;
      }
      case 'set_trigger_mode': c.pads[a.pad].trigger_mode[a.side] = a.mode; changed(); return null;
      case 'clear_pad': {
        const p = c.pads[a.pad];
        for (const e of Object.values(p.buttons)) { delete e.value; e.type = 'unbound'; }
        p.quick_l = {}; p.quick_r = {};
        changed(); return null;
      }
      case 'reset_pad': c.pads[a.pad] = clone(defaults[a.pad]); changed(); return null;
      case 'set_pad_tables': c.pads[a.pad] = clone(a.tables); changed(); return null;
      case 'set_macro': c.macros[a.name] = clone(a.def); changed(); return null;
      case 'rename_macro':
        if (c.macros[a.new]) throw `macro '${a.new}' already exists`;
        c.macros[a.new] = c.macros[a.old]; delete c.macros[a.old]; renameRefs(a.old, a.new); changed(); return null;
      case 'delete_macro': delete c.macros[a.name]; renameRefs(a.name, null); changed(); return null;
      case 'set_settings': Object.assign(c, clone(a.settings)); changed(); return null;
      case 'set_mapping_enabled': c.mapping_enabled[a.kind] = a.enabled; changed(); return null;
      case 'set_rumble_on_connect': c.rumble_on_connect = a.enabled; changed(); return null;
      case 'save_stick_cal': c.stick_cal = c.stick_cal || {}; c.stick_cal[a.pad] = clone(a.cal); changed(); return null;
      case 'reset_stick_cal': if (c.stick_cal) delete c.stick_cal[a.pad]; changed(); return null;
      default: return null;
    }
  }

  window.__TAURI__ = {
    core: { invoke },
    event: {
      listen: async (name, cb) => {
        const id = nextId++;
        (listeners[name] = listeners[name] || []).push({ id, cb });
        return () => { listeners[name] = (listeners[name] || []).filter(l => l.id !== id); };
      },
    },
  };
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

const calls = page => page.evaluate(() => window.__mockCalls);
const lastCall = async (page, cmd) => (await calls(page)).filter(c => c.cmd === cmd).pop();
const countCalls = async (page, cmd) => (await calls(page)).filter(c => c.cmd === cmd).length;
const settle = page => page.waitForTimeout(120);
const slot = (page, key) => page.locator(`#board .slot[data-slot="${key}"]`);
const tile = (page, key) => page.locator(`#board [data-tile="${key}"]`);
const slotText = async (page, key) => ((await slot(page, key).locator('.val').textContent()) || '').trim();

// ─── Main ────────────────────────────────────────────────────────────────────

const server = await startServer();
const base = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch({ executablePath: args.chrome, headless: true });
const consoleErrors = [];
const pageErrors = [];
const dialogs = [];

try {
  const context = await browser.newContext({ viewport: { width: 980, height: 640 } });
  await context.addInitScript(tauriMock, { config: exampleConfig, version: appVersion });
  const page = await context.newPage();
  page.on('pageerror', e => pageErrors.push(String(e && (e.stack || e))));
  page.on('console', m => {
    if (m.type() !== 'error') return;
    const url = (m.location() && m.location().url) || '';
    if (/Failed to load resource/.test(m.text()) && /favicon\.ico/.test(url)) return;
    consoleErrors.push(`${m.text()} @ ${url}`);
  });
  page.on('dialog', async d => { dialogs.push(`${d.type()}: ${d.message()}`); await d.dismiss(); });

  await page.goto(base, { waitUntil: 'load' });
  await page.locator('#board .hdr').waitFor({ timeout: 10000 });
  await page.waitForFunction(() => document.querySelectorAll('#board .slot').length > 20, null, { timeout: 10000 });
  await settle(page);
  await page.screenshot({ path: path.join(outDir, 'web-main.png') });

  // ── Task 5: header, pads, binding actions, undo ───────────────────────────
  check('header title 按鍵設定', ((await page.locator('#board .hdr .ttl').textContent()) || '').trim() === '按鍵設定');
  const padNames = await page.locator('#board .hdr [data-pad]').allTextContents();
  check('three pad buttons', JSON.stringify(padNames.map(s => s.trim())) === JSON.stringify(['DualSense', 'Switch Pro', 'Switch 2 Pro']), padNames.join('|'));
  const lights = await page.locator('#board .hdr [data-pad] .live').evaluateAll(els => els.map(e => e.classList.contains('idle') ? 'idle' : 'live'));
  check('DualSense light live, others idle', JSON.stringify(lights) === '["live","idle","idle"]', lights.join(','));
  check('one ⏻ (connected pad only)', (await page.locator('#board .hdr .pwr').count()) === 1);

  await page.click('#board [data-pad="ns2_pro"]');
  await settle(page);
  const ns2Slots = await page.locator('#board .slot').evaluateAll(els => els.map(e => e.dataset.slot));
  check('Switch 2 Pro shows 30/31/32 and no touchpad slots', ['30', '31', '32'].every(k => ns2Slots.includes(k)) && !ns2Slots.includes('25'));
  check('pad choice saved as last_pad', (await lastCall(page, 'set_ui_prefs'))?.args?.prefs?.last_pad === 'ns2_pro');

  await tile(page, 'Q').dragTo(slot(page, '0'));
  await settle(page);
  const bindQ = await lastCall(page, 'set_binding');
  check('drag Q onto slot 0 → set_binding (ns2_pro, base, 0, key q)',
    bindQ?.args?.pad === 'ns2_pro' && bindQ.args.page === 'base' && bindQ.args.id === 0 && bindQ.args.entry.type === 'key' && bindQ.args.entry.value === 'q',
    JSON.stringify(bindQ?.args));
  check('slot 0 shows Q', (await slotText(page, '0')) === 'Q', await slotText(page, '0'));

  const swaps = await countCalls(page, 'swap_bindings');
  await slot(page, '0').dragTo(slot(page, '1'));
  await settle(page);
  const sw = await lastCall(page, 'swap_bindings');
  check('drag slot 0 onto slot 1 → one swap_bindings', (await countCalls(page, 'swap_bindings')) === swaps + 1 &&
    sw.args.pageA === 'base' && sw.args.idA === 0 && sw.args.pageB === 'base' && sw.args.idB === 1, JSON.stringify(sw?.args));
  check('after swap slot 1 shows Q', (await slotText(page, '1')) === 'Q');

  await slot(page, '1').dragTo(page.locator('#board .stage'), { targetPosition: { x: 300, y: 12 } });
  await settle(page);
  const del = await lastCall(page, 'set_binding');
  check('drag slot out → unbound', del?.args?.id === 1 && del.args.entry.type === 'unbound', JSON.stringify(del?.args));

  await slot(page, '11').dblclick();
  await settle(page);
  const dbl = await lastCall(page, 'set_binding');
  check('double-click clears a slot', dbl?.args?.id === 11 && dbl.args.entry.type === 'unbound', JSON.stringify(dbl?.args));

  await page.click('#board .qcap [data-qs="L"][data-mode="single"]');
  await settle(page);
  const tm = await lastCall(page, 'set_trigger_mode');
  check('單鍵 → set_trigger_mode (l, key)', tm?.args?.pad === 'ns2_pro' && tm.args.side === 'l' && tm.args.mode === 'key', JSON.stringify(tm?.args));
  check('ZL page folds into slot 23', (await slot(page, '23').count()) === 1 && (await slot(page, 'L11').count()) === 0);

  await page.click('#board [data-act="clear"]');
  await settle(page);
  check('全部清除 → clear_pad', (await lastCall(page, 'clear_pad'))?.args?.pad === 'ns2_pro');
  check('復原 enabled after a change', await page.locator('#board [data-act="undo"]').isEnabled());
  await page.click('#board [data-act="undo"]');
  await settle(page);
  const undo = await lastCall(page, 'set_pad_tables');
  check('復原 → set_pad_tables restores the table before 全部清除',
    undo?.args?.pad === 'ns2_pro' && undo.args.tables.buttons['15'].type === 'key', JSON.stringify(undo?.args?.tables?.buttons?.['15']));
  check('重做 enabled after undo', await page.locator('#board [data-act="redo"]').isEnabled());
  await page.click('#board [data-act="redo"]');
  await settle(page);
  const redo = await lastCall(page, 'set_pad_tables');
  check('重做 → set_pad_tables with the cleared table', redo?.args?.tables?.buttons?.['15']?.type === 'unbound');
  check('重做 disabled at the end', !(await page.locator('#board [data-act="redo"]').isEnabled()));

  await page.click('#board [data-act="reset"]');
  await settle(page);
  check('恢復預設 → reset_pad', (await lastCall(page, 'reset_pad'))?.args?.pad === 'ns2_pro');

  // Review focus 2: undo switches to the pad it changes.
  await page.click('#board [data-pad="dualsense"]');
  await settle(page);
  await tile(page, 'W').dragTo(slot(page, '2'));
  await settle(page);
  await page.click('#board [data-pad="ns_pro"]');
  await settle(page);
  await page.keyboard.press('Control+z');
  await settle(page);
  const ux = await lastCall(page, 'set_pad_tables');
  const shownPad = await page.locator('#board .hdr [data-pad][aria-pressed="true"]').getAttribute('data-pad');
  check('undo switches to the pad it changes (Ctrl+Z)', ux?.args?.pad === 'dualsense' && shownPad === 'dualsense', `${ux?.args?.pad} ${shownPad}`);

  // Review focus 1: an external config-changed keeps the selected pad.
  await page.click('#board [data-pad="ns2_pro"]');
  await settle(page);
  await page.evaluate(() => {
    window.__mock.config.pads.ns2_pro.buttons['9'] = { label: 'L1', type: 'key', value: 'F7' };
    window.__mockEmit('config-changed', { source: 'watcher' });
  });
  await settle(page);
  const stillNs2 = await page.locator('#board .hdr [data-pad][aria-pressed="true"]').getAttribute('data-pad');
  check('external config-changed keeps the selected pad', stillNs2 === 'ns2_pro' && (await slotText(page, '9')) === 'F7', `${stillNs2} ${await slotText(page, '9')}`);

  // Failed undo: the entry is dropped and nothing else changes.
  await tile(page, 'E').dragTo(slot(page, '2'));
  await settle(page);
  await page.evaluate(() => { window.__mockFail.set_pad_tables = 'unknown macro "gone"'; });
  await page.click('#board [data-act="undo"]');
  await settle(page);
  const note = ((await page.locator('#board .ftr .note').textContent()) || '').trim();
  check('a refused undo shows the reason', /無法復原/.test(note) && /gone/.test(note), note);
  check('a refused undo leaves the slot as it was', (await slotText(page, '2')) === 'E');

  // Header switches and ⏻.
  await page.click('#board [data-pad="dualsense"]');
  await settle(page);
  await page.click('#board .hdr .pwr');
  await settle(page);
  check('⏻ → disconnect_gamepad (dualsense)', (await lastCall(page, 'disconnect_gamepad'))?.args?.pad === 'dualsense');
  await page.click('#board [data-act="rumble"]');
  await settle(page);
  check('連線震動 → set_rumble_on_connect false', (await lastCall(page, 'set_rumble_on_connect'))?.args?.enabled === false);
  await page.click('#board [data-act="map"]');
  await settle(page);
  const me = await lastCall(page, 'set_mapping_enabled');
  check('鍵鼠對應 → set_mapping_enabled (dualsense, false)', me?.args?.kind === 'dualsense' && me.args.enabled === false, JSON.stringify(me?.args));
  check('mapping off dims the board', await page.locator('#board.mock.off, #board .mock.off').count() > 0 || await page.evaluate(() => document.getElementById('board').classList.contains('off')));
  await page.click('#board [data-act="map"]');
  await settle(page);

  // ── Layout: the window never scrolls; the board always fits ──────────────
  for (const [w, h] of [[880, 560], [980, 640], [1280, 800], [1600, 1000]]) {
    await page.setViewportSize({ width: w, height: h });
    await page.waitForTimeout(150);
    const m = await page.evaluate(() => {
      const r = document.getElementById('board').getBoundingClientRect();
      const d = document.documentElement;
      return { sw: d.scrollWidth, cw: d.clientWidth, sh: d.scrollHeight, ch: d.clientHeight, l: r.left, t: r.top, r: r.right, b: r.bottom, iw: innerWidth, ih: innerHeight };
    });
    check(`no scroll and board fits @${w}x${h}`,
      m.sw === m.cw && m.sh === m.ch && m.l >= -0.5 && m.t >= -0.5 && m.r <= m.iw + 0.5 && m.b <= m.ih + 0.5, JSON.stringify(m));
  }
  await page.setViewportSize({ width: 980, height: 640 });
  const ob = await page.evaluate(() => ({
    html: getComputedStyle(document.documentElement).overscrollBehaviorX,
    body: getComputedStyle(document.body).overscrollBehaviorX,
  }));
  check('overscroll-behavior none on html/body', ob.html === 'none' && ob.body === 'none', JSON.stringify(ob));

  // ── Task 6: live feedback and key capture ─────────────────────────────────
  await page.click('#board [data-pad="dualsense"]');
  await settle(page);
  await page.evaluate(() => window.__mockEmit('button-down', { id: 12 }));
  await settle(page);
  check('button-down lights its slot and its line', (await slot(page, '12').getAttribute('class')).includes('hit') &&
    (await page.locator('#board .lines g.hit').count()) === 1);
  check('button-down lights the bound tile', (await tile(page, '↓').getAttribute('class')).includes('hit'));
  await page.evaluate(() => window.__mockEmit('button-up', { id: 12 }));
  await settle(page);
  check('button-up clears the highlight', !(await slot(page, '12').getAttribute('class')).includes('hit') &&
    (await page.locator('#board .lines g.hit').count()) === 0);
  await page.evaluate(() => { window.__mockEmit('button-down', { id: 23 }); window.__mockEmit('button-down', { id: 13 }); });
  await settle(page);
  check('page-mode L2 held: D-pad press lights the quick_l slot', (await slot(page, 'L13').getAttribute('class')).includes('hit') &&
    !(await slot(page, '13').getAttribute('class')).includes('hit'));
  await page.evaluate(() => { window.__mockEmit('button-up', { id: 13 }); window.__mockEmit('button-up', { id: 23 }); });
  await settle(page);
  await page.evaluate(() => window.__mockEmit('activity', { ts_ms: Date.now(), kind: 'emit', key: 'q', action: 'down' }));
  await settle(page);
  check('emitted key lights its tile', (await tile(page, 'Q').getAttribute('class')).includes('hit'));
  await page.evaluate(() => window.__mockEmit('activity', { ts_ms: Date.now(), kind: 'emit', key: 'q', action: 'up' }));
  await settle(page);
  check('emitted key-up clears its tile', !(await tile(page, 'Q').getAttribute('class')).includes('hit'));

  await page.keyboard.down('ArrowUp');
  await page.waitForTimeout(80);
  check('reverse lookup: a real key lights its tile and the slots bound to it',
    (await tile(page, '↑').getAttribute('class')).includes('hit') && (await slot(page, '11').getAttribute('class')).includes('hit') &&
    (await slot(page, '15').getAttribute('class')).includes('hit'));
  await page.keyboard.up('ArrowUp');
  await settle(page);
  check('reverse lookup clears on key-up', !(await slot(page, '11').getAttribute('class')).includes('hit'));
  await page.evaluate(() => {
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW', key: 'w', bubbles: true }));
    setTimeout(() => window.__mockEmit('activity', { ts_ms: Date.now(), kind: 'emit', key: 'w', action: 'down' }), 5);
    setTimeout(() => window.__mockEmit('activity', { ts_ms: Date.now(), kind: 'emit', key: 'w', action: 'up' }), 15);
  });
  await page.waitForTimeout(100);   // past the 30 ms window; the real key is still "down"
  const synthLit = (await tile(page, 'W').getAttribute('class')).includes('hit');
  await page.evaluate(() => document.dispatchEvent(new KeyboardEvent('keyup', { code: 'KeyW', key: 'w', bubbles: true })));
  await settle(page);
  check('reverse lookup ignores the engine\'s own synthesized key', synthLit === false, 'W lit after the engine typed it: ' + synthLit);

  const capBefore = await countCalls(page, 'set_capture_active');
  await slot(page, '3').click();
  await settle(page);
  check('clicking a slot starts key capture', (await lastCall(page, 'set_capture_active'))?.args?.active === true &&
    (await countCalls(page, 'set_capture_active')) === capBefore + 1);
  check('capture bar text is exact', ((await page.locator('#board .capbar').textContent()) || '').trim() === '按鍵鼠上的鍵，或點下面的方塊（Esc 取消）');
  check('waiting slot has the keycap badge', (await slot(page, '3').locator('.kc').count()) === 1);
  await page.keyboard.press('f');
  await settle(page);
  const capBind = await lastCall(page, 'set_binding');
  check('pressed key binds the waiting slot', capBind?.args?.id === 3 && capBind.args.entry.value === 'f', JSON.stringify(capBind?.args));
  check('capture ends after binding', (await lastCall(page, 'set_capture_active'))?.args?.active === false && (await page.locator('#board .capbar').count()) === 0);
  await slot(page, '2').click();
  await settle(page);
  await slot(page, '2').click({ button: 'right' });
  await settle(page);
  const rc = await lastCall(page, 'set_binding');
  check('right click while waiting binds 右鍵', rc?.args?.id === 2 && rc.args.entry.type === 'mouse' && rc.args.entry.value === 'right', JSON.stringify(rc?.args));
  await slot(page, '1').click();
  await settle(page);
  await page.mouse.move(470, 500);
  await page.mouse.wheel(0, -120);
  await settle(page);
  const wh = await lastCall(page, 'set_binding');
  check('wheel up while waiting binds 滾輪上', wh?.args?.id === 1 && wh.args.entry.value === 'wheel-up', JSON.stringify(wh?.args));
  const nBind = await countCalls(page, 'set_binding');
  await slot(page, '0').click();
  await settle(page);
  await page.keyboard.press('Escape');
  await settle(page);
  check('Esc cancels capture without binding', (await countCalls(page, 'set_binding')) === nBind &&
    (await lastCall(page, 'set_capture_active'))?.args?.active === false);
  await slot(page, '0').click();
  await settle(page);
  await page.click('#board [data-pad="ns_pro"]');
  await settle(page);
  const endPad = (await lastCall(page, 'set_capture_active'))?.args?.active === false && (await page.locator('#board .capbar').count()) === 0;
  await slot(page, '0').click();
  await settle(page);
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await settle(page);
  const endBlur = (await lastCall(page, 'set_capture_active'))?.args?.active === false && (await page.locator('#board .capbar').count()) === 0;
  check('capture ends on pad switch and on blur', endPad && endBlur && (await countCalls(page, 'set_binding')) === nBind, `${endPad} ${endBlur}`);
  await page.click('#board [data-pad="dualsense"]');
  await settle(page);

  // ── Task 7: macro box and editor ──────────────────────────────────────────
  await page.evaluate(() => {
    const m = window.__mock.config.macros;
    for (const n of ['a1', 'a2', 'a3', 'a4', 'a5', 'a6', 'a7', 'a8', 'a9']) m[n] = { loop: false, steps: [{ key: 'z', action: 'down', delay_ms: [40, 60] }, { key: 'z', action: 'up', delay_ms: [15, 25] }] };
    window.__mockEmit('config-changed', { source: 'watcher' });
  });
  await settle(page);
  check('macro box page 1: 8 macro tiles, no ＋', (await page.locator('#board .macros .tile.mac').count()) === 8 && (await page.locator('#board .macros .mnew').count()) === 0);
  await page.locator('#board .macros').hover();
  await page.mouse.wheel(0, 120);
  await settle(page);
  check('wheel turns the macro page: 1 tile + ＋', (await page.locator('#board .macros .tile.mac').count()) === 1 && (await page.locator('#board .macros .mnew').count()) === 1);
  await page.click('#board .macros .mnew');
  await settle(page);
  const nm = await lastCall(page, 'set_macro');
  check('＋ creates 巨集 1 with no steps', nm?.args?.name === '巨集 1' && nm.args.def.steps.length === 0 && nm.args.def.loop === false, JSON.stringify(nm?.args));
  check('the editor opens on the new macro', (await page.locator('#board .med').count()) === 1 && (await page.locator('#board .med .mname').inputValue()) === '巨集 1');
  check('mouse and macro tiles are locked while editing', (await page.locator('#board .m3s.locked').count()) === 1 && (await page.locator('#board .macros.locked').count()) === 1);
  await tile(page, 'A').click();
  await settle(page);
  const stepsA = await page.locator('#board .med .st[data-step]:not([data-step="end"])').count();
  const pills = await page.locator('#board .med [data-act="sdelay"]').allTextContents();
  check('a keyboard click appends a down+up pair with default delays', stepsA === 2 && pills[0].includes('40–60') && pills[1].includes('15–25'), pills.join('|'));
  await page.click('#board .med [data-act="sdelay"][data-i="0"]');
  await page.locator('#board .dpop [data-f="dhi"]').fill('40');
  await page.locator('#board .dpop [data-f="dhi"]').dispatchEvent('input');
  await page.click('#board .med .mhd .mname');
  const before = await countCalls(page, 'set_macro');
  await page.click('#board .med [data-act="mdone"]');
  await settle(page);
  check('完成 is refused while a delay has min >= max (its popover opens)', (await countCalls(page, 'set_macro')) === before &&
    (await page.locator('#board .dpop').count()) === 1 && (await page.locator('#board .med').count()) === 1);
  await page.locator('#board .dpop [data-f="dhi"]').fill('70');
  await page.locator('#board .dpop [data-f="dhi"]').dispatchEvent('input');
  await page.keyboard.press('Escape');
  await page.locator('#board .med .mname').fill('連發');
  await page.locator('#board .med .mname').dispatchEvent('input');
  await page.click('#board .med [data-act="mloop"]');
  await page.click('#board .med [data-act="mdone"]');
  await settle(page);
  const seq = (await calls(page)).map(c => c.cmd).filter(c => /macro/.test(c)).slice(-2);
  const saved = await lastCall(page, 'set_macro');
  check('rename + 完成 → rename_macro then set_macro', seq.join(',') === 'rename_macro,set_macro' &&
    saved.args.name === '連發' && saved.args.def.loop === true && saved.args.def.steps[0].key === 'a' &&
    JSON.stringify(saved.args.def.steps[0].delay_ms) === '[40,70]', seq.join(',') + ' ' + JSON.stringify(saved?.args));
  check('the editor closes after 完成', (await page.locator('#board .med').count()) === 0);
  await page.locator('#board [data-medit="連發"]').click();
  await settle(page);
  await page.click('#board .med [data-act="mdel"]');
  await settle(page);
  check('trash → delete_macro', (await lastCall(page, 'delete_macro'))?.args?.name === '連發' && (await page.locator('#board .med').count()) === 0);
  await page.evaluate(() => {
    const c = window.__mock.config;
    c.macros['<b>x"y</b>'] = { loop: false, steps: [] };
    c.pads.dualsense.buttons['0'] = { label: 'Cross', type: 'macro', value: '<b>x"y</b>' };
    window.__mockEmit('config-changed', { source: 'watcher' });
  });
  await settle(page);
  await page.click('#board .macros .mnew');
  await settle(page);
  await page.click('#board .med [data-act="mloop"]');
  const nSet = await countCalls(page, 'set_macro');
  await page.click('#board .med [data-act="mdone"]');
  await settle(page);
  check('完成 refuses a looping macro with no steps', (await countCalls(page, 'set_macro')) === nSet &&
    /至少一步/.test((await page.locator('#board .med .merr').textContent()) || ''));
  await page.click('#board .med [data-act="mdel"]');
  await settle(page);
  await page.evaluate(() => {
    window.__mock.config.macros['macro_attack_combo'] = { loop: false, steps: [{ key: 'z', action: 'down', delay_ms: [40, 60] }, { key: 'z', action: 'up', delay_ms: [15, 25] }] };
    window.__mockEmit('config-changed', { source: 'watcher' });
  });
  await settle(page);
  const nRename = await countCalls(page, 'rename_macro');
  await page.locator('#board [data-medit="macro_attack_combo"]').click();
  await settle(page);
  await page.click('#board .med [data-act="mdone"]');
  await settle(page);
  check('完成 on an untouched long-named macro does not rename it', (await countCalls(page, 'rename_macro')) === nRename &&
    (await lastCall(page, 'set_macro'))?.args?.name === 'macro_attack_combo', JSON.stringify((await lastCall(page, 'set_macro'))?.args?.name));
  check('macro name is escaped in tile and slot', (await page.locator('#board b').count()) === 0 && (await slotText(page, '0')) === '<b>x"y</b>');

  // ── Task 8: settings window ───────────────────────────────────────────────
  await page.click('#board [data-act="setopen"]');
  await settle(page);
  check('gear opens the settings window', (await page.locator('#board .setw').count()) === 1);
  const tabs = await page.locator('#board .stabs [data-stab]').allTextContents();
  check('four tabs 一般/搖桿/觸控板/按鍵', JSON.stringify(tabs.map(t => t.trim())) === '["一般","搖桿","觸控板","按鍵"]', tabs.join('|'));
  check('一般 shows the config path and both open buttons', /dualsense-mapper\.json/.test(await page.locator('#board .cfgp').textContent()) &&
    (await page.locator('#board [data-act="opencfg"]').count()) === 1 && (await page.locator('#board [data-act="opendir"]').count()) === 1);
  await page.click('#board [data-act="opencfg"]');
  await page.click('#board [data-act="opendir"]');
  await settle(page);
  check('用記事本開啟 / 開啟所在資料夾 invoke their commands', (await countCalls(page, 'open_config_in_editor')) === 1 && (await countCalls(page, 'open_config_folder')) === 1);
  for (let i = 0; i < 8; i++) {
    await page.evaluate(i => window.__mockEmit('activity', { ts_ms: Date.now(), kind: 'emit', key: 'k' + i, action: i % 2 ? 'up' : 'down' }), i);
  }
  await settle(page);
  const rows = await page.locator('#board #logrows .lr').allTextContents();
  check('最近的事件: at most 6 rows, newest first', rows.length === 6 && /K7/i.test(rows[0]) && /放開/.test(rows[0]), rows.slice(0, 2).join(' | '));
  for (const t of ['stick', 'tp', 'timing', 'gen']) {
    await page.click(`#board [data-stab="${t}"]`);
    await settle(page);
  }
  check('every tab switches its pane', (await page.locator('#board [data-stab="gen"][aria-pressed="true"]').count()) === 1);
  await page.click('#board [data-stab="timing"]');
  await page.locator('#board [data-f="press0"]').fill('25');
  await page.locator('#board [data-f="press0"]').dispatchEvent('input');
  await page.locator('#board [data-f="press1"]').fill('25');
  await page.locator('#board [data-f="press1"]').dispatchEvent('input');
  await page.click('#board [data-stab="gen"]');
  const nSettings = await countCalls(page, 'set_settings');
  await page.click('#board [data-act="setsave"]');
  await settle(page);
  check('invalid 按住維持下限 is refused: message, outline, its tab', (await countCalls(page, 'set_settings')) === nSettings &&
    /按住維持下限/.test(await page.locator('#board .setw .err').textContent()) &&
    (await page.locator('#board [data-f="press0"][aria-invalid="true"]').count()) === 1 &&
    (await page.locator('#board [data-stab="timing"][aria-pressed="true"]').count()) === 1);
  await page.locator('#board [data-f="press1"]').fill('30');
  await page.locator('#board [data-f="press1"]').dispatchEvent('input');
  await page.click('#board [data-stab="stick"]');
  await page.locator('#board [data-f="deadzone"]').fill('0.6');
  await page.locator('#board [data-f="deadzone"]').dispatchEvent('input');
  await page.click('#board [data-act="setsave"]');
  await settle(page);
  const ss = await lastCall(page, 'set_settings');
  check('儲存 sends set_settings with the edits', Math.abs(ss?.args?.settings?.deadzone - 0.6) < 1e-6 &&
    JSON.stringify(ss.args.settings.min_press_ms) === '[25,30]' && (await page.locator('#board .setw').count()) === 0, JSON.stringify(ss?.args?.settings));
  await page.click('#board [data-act="setopen"]');
  await settle(page);
  await page.click('#board [data-stab="stick"]');
  await page.locator('#board [data-f="deadzone"]').fill('0.2');
  await page.locator('#board [data-f="deadzone"]').dispatchEvent('input');
  const nS2 = await countCalls(page, 'set_settings');
  await page.keyboard.press('Escape');
  await settle(page);
  check('Esc closes without saving', (await page.locator('#board .setw').count()) === 0 && (await countCalls(page, 'set_settings')) === nS2);
  await page.click('#board [data-act="setopen"]');
  await settle(page);
  await page.click('#board [data-stab="tp"]');
  await page.evaluate(() => window.__mockEmit('touchpad-click', { raw_x: 1200, raw_y: 300, quadrant: 26 }));
  await settle(page);
  check('touchpad dot follows the finger', (await page.locator('#board .tpad circle').count()) === 1);
  await page.click('#board [data-act="tpmid"]');
  await settle(page);
  check('中心設在手指位置 copies the finger position', (await page.locator('#board [data-f="midX"]').inputValue()) === '1200' &&
    (await page.locator('#board [data-f="midY"]').inputValue()) === '300');
  await page.click('#board .setw .x');
  await settle(page);
  check('✕ closes the window', (await page.locator('#board .setw').count()) === 0);

  // @@NEXT-TASKS

  const inPageErrors = await page.evaluate(() => window.__mockErrors);
  const otherMisses = httpMisses.filter(p => p !== '/favicon.ico');
  check('zero pageerror', pageErrors.length === 0, pageErrors.join(' | '));
  check('served under the app CSP', csp.includes("script-src 'self'"), csp);
  check('zero console errors', consoleErrors.length === 0, consoleErrors.join(' | '));
  check('zero window error / unhandledrejection', inPageErrors.length === 0, inPageErrors.join(' | '));
  check('no unexpected dialogs (alert/confirm/prompt)', dialogs.length === 0, dialogs.join(' | '));
  check('no 404 except favicon', otherMisses.length === 0, otherMisses.join(' '));
  const seen = [...new Set((await calls(page)).map(c => c.cmd))].sort();
  console.log(`info: IPC calls seen: ${seen.join(', ')}`);
} catch (e) {
  check('click-through ran to completion', false, String(e && (e.stack || e)));
} finally {
  await browser.close();
  server.close();
}

const failed = checks.filter(c => !c.ok).length;
console.log(`web-smoke: ${checks.length - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
