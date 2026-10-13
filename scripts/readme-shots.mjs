// readme-shots.mjs — regenerate the README screenshots in docs/images/readme/.
//
// Same approach as scripts/verify/web-smoke.mjs: serve rust/web/, inject a
// mock window.__TAURI__ before the module scripts run, and drive the page
// with Playwright + the cached Chromium (WebView2 is Chromium too). The mock
// serves a demo config (rust/config.example.json plus a few bindings and one
// macro), so the pictures never show anyone's real setup.
//
//   NODE_PATH=rust/target/verify/node/node_modules node scripts/readme-shots.mjs \
//     --chrome ~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome
//
// (scripts/verify.sh --only web installs playwright-core into
// rust/target/verify/node the first time.)

import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core');

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const webDir = path.join(repo, 'rust/web');
const outDir = path.join(repo, 'docs/images/readme');
const argv = process.argv.slice(2);
const chrome = argv[argv.indexOf('--chrome') + 1];
if (!chrome || argv.indexOf('--chrome') < 0) throw new Error('missing --chrome <path>');
const version = /^version = "([^"]+)"/m.exec(fs.readFileSync(path.join(repo, 'rust/Cargo.toml'), 'utf8'))[1];
fs.mkdirSync(outDir, { recursive: true });

// ─── Demo config ─────────────────────────────────────────────────────────────

const config = JSON.parse(fs.readFileSync(path.join(repo, 'rust/config.example.json'), 'utf8'));
const ds = config.pads.dualsense.buttons;
const key = (id, value) => { ds[String(id)] = { label: ds[String(id)].label, type: 'key', value }; };
key(0, 'Alt'); key(1, 'z'); key(2, 'Shift'); key(3, 'a');
key(5, 'Space'); key(6, 'Enter'); key(9, 'q'); key(10, 'e');
ds['24'] = { label: ds['24'].label, type: 'macro', value: 'jump_attack' };
config.macros = {
  jump_attack: {
    loop: true,
    steps: [
      { key: 'Alt', action: 'down', delay_ms: [40, 60] },
      { key: 'Alt', action: 'up', delay_ms: [15, 25] },
      { key: 'z', action: 'down', delay_ms: [40, 60] },
      { key: 'z', action: 'up', delay_ms: [120, 180] },
    ],
  },
};

// ─── Static server ───────────────────────────────────────────────────────────

const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png',
};
const server = http.createServer((req, res) => {
  const p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = path.resolve(webDir, p === '/' ? 'index.html' : p.replace(/^\/+/, ''));
  if (!file.startsWith(webDir + path.sep) || !fs.existsSync(file)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'Content-Type': MIME[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise(r => server.listen(0, '127.0.0.1', r));

// ─── Tauri mock (runs in the page) ───────────────────────────────────────────

function tauriMock({ config, version }) {
  const listeners = {};
  let nextId = 1;
  const status = {
    name: 'DualSense Wireless Controller', transport: 'BT', kind: 'dualsense',
    warnings: [], connected: ['dualsense'],
  };
  window.__emit = (name, payload) => {
    for (const l of (listeners[name] || []).slice()) l.cb({ event: name, id: l.id, payload });
  };
  const clone = v => JSON.parse(JSON.stringify(v));
  async function invoke(cmd) {
    switch (cmd) {
      case 'get_controller_status': return clone(status);
      case 'get_config': return clone(config);
      case 'get_ui_prefs': return { drawer_open: false, last_tab: null };
      case 'get_app_version': return version;
      default: return null;
    }
  }
  window.__TAURI__ = {
    core: { invoke },
    event: {
      listen: async (name, cb) => {
        const id = nextId++;
        (listeners[name] = listeners[name] || []).push({ id, cb });
        return () => { listeners[name] = listeners[name].filter(l => l.id !== id); };
      },
    },
  };
}

// ─── Shots ───────────────────────────────────────────────────────────────────

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const errors = [];
try {
  const context = await browser.newContext({ viewport: { width: 1100, height: 720 }, deviceScaleFactor: 2 });
  await context.addInitScript(tauriMock, { config, version });
  const page = await context.newPage();
  page.on('pageerror', e => errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => document.querySelectorAll('#chip-list .chip-row').length === 33);
  const shot = async (name, clipHeight) => {
    await page.waitForTimeout(250);
    const clip = clipHeight ? { x: 0, y: 0, width: 1100, height: clipHeight } : undefined;
    await page.screenshot({ path: path.join(outDir, name), clip });
    console.log(`wrote docs/images/readme/${name}`);
  };

  // 1. Mappings, controller connected.
  await shot('mappings.png');

  // 2. Bind popup on Triangle, Key segment.
  await page.locator('#chip-list .chip-row[data-id="3"]').click();
  await page.locator('.bind-popup-overlay').waitFor();
  await shot('bind-popup.png');
  await page.locator('.bind-popup-overlay .bp-cancel').click();
  await page.locator('.bind-popup-overlay').waitFor({ state: 'detached' });

  // 3. Macros tab with the demo macro open.
  await page.click('[data-tab="macros"]');
  await page.locator('#pane-macros').getByText('jump_attack').first().click();
  await shot('macros.png', 380);

  // 4. Settings tab.
  await page.click('[data-tab="settings"]');
  await shot('settings.png');

  // 5. Activity log drawer over the Mappings tab.
  await page.click('[data-tab="mappings"]');
  await page.click('#btn-activity');
  // Cross taps Alt, then holding R2 runs the jump_attack macro. Button
  // events go through button-down/up (the drawer logs them itself).
  await page.evaluate(async () => {
    const seq = [
      ['button-down', { id: 0 }], ['activity', { kind: 'emit', key: 'Alt', action: 'down' }],
      ['button-up', { id: 0 }], ['activity', { kind: 'emit', key: 'Alt', action: 'up' }],
      ['button-down', { id: 24 }], ['activity', { kind: 'macro-start', name: 'jump_attack' }],
      ['activity', { kind: 'emit', key: 'Alt', action: 'down' }],
      ['activity', { kind: 'emit', key: 'Alt', action: 'up' }],
      ['activity', { kind: 'emit', key: 'z', action: 'down' }],
      ['activity', { kind: 'emit', key: 'z', action: 'up' }],
      ['button-up', { id: 24 }],
    ];
    for (const [name, p] of seq) {
      window.__emit(name, name === 'activity' ? { ts_ms: Date.now(), ...p } : p);
      await new Promise(r => setTimeout(r, 60));
    }
  });
  await shot('activity.png');
} finally {
  await browser.close();
  server.close();
}
if (errors.length) { console.error(errors.join('\n')); process.exit(1); }
