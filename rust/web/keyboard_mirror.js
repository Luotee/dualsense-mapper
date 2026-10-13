// Mirror keyboard-press visual feedback onto the in-app controller SVG.
//
// Defer-and-check 30ms: on keydown, schedule a mirror flash 30ms later.
// If any 'button-down' for an id ∈ bindingsByKey[name] arrives in that
// window, the keydown is treated as our own engine synth and skipped.
// Otherwise it's a real keyboard input and flashes every bound id.
//
// 30ms is below 2 frames @ 60Hz so the deferral is imperceptible. It's
// big enough to cover IPC arrival jitter (~10-20ms) between SendInput
// → OS → window keydown and HID worker → channel → Tauri emit → JS.
//
// Suppression rules unchanged: ev.repeat, capture active, form fields.

import { invoke, listen } from './ipc.js';
import { resolveKeyName } from './key_capture.js';
import { isCaptureActive } from './capture_state.js';
import * as controller from './controller.js';
import * as padView from './pad_view.js';

const DEFER_MS = 30;

let bindingsByKey       = new Map();   // key-name → array of numeric button ids
let keyByButtonId       = new Map();   // numeric id → key-name
const activeKeys        = new Set();   // canonical names currently lit
const recentButtonDowns = new Map();   // numeric id → performance.now() of last 'button-down'
const pendingKeyDowns   = new Map();   // key name → setTimeout handle

function currentSvg() {
  return document.querySelector('#controller-host svg.controller');
}

export async function init() {
  await refresh();
  listen('config-changed', refresh);
  padView.onPadChange(refresh);
  listen('button-down', ev => recentButtonDowns.set(Number(ev.id), performance.now()));
  document.addEventListener('keydown', onKeyDown);
  document.addEventListener('keyup',   onKeyUp);
  // GC stale entries every 1s. Anything older than 1s can't possibly
  // match a future keydown's 30ms window.
  setInterval(() => {
    const cutoff = performance.now() - 1000;
    for (const [id, ts] of recentButtonDowns) {
      if (ts < cutoff) recentButtonDowns.delete(id);
    }
  }, 1000);
}

// Generic modifier names in the config (e.g. "Alt", "Shift", "Control")
// expand to both side-specific physical keys, since `resolveKeyName`
// always reports the side a physical press came from ("LAlt", never
// plain "Alt"). All lookup keys are stored lowercase so a hand-edited
// config with "lalt" or "LALT" still matches.
const MOD_ALIASES_LOWER = {
  'alt':     ['lalt', 'ralt'],
  'shift':   ['lshift', 'rshift'],
  'control': ['lcontrol', 'rcontrol'],
  'ctrl':    ['lcontrol', 'rcontrol'],
};

async function refresh() {
  try {
    const cfg = await invoke('get_config');
    const m = new Map();
    const k = new Map();
    for (const [id, entry] of Object.entries(padView.baseButtons(cfg, padView.currentPad()))) {
      if (entry?.type !== 'key' || !entry?.value) continue;
      const lower = entry.value.toLowerCase();
      const lookupKeys = MOD_ALIASES_LOWER[lower] || [lower];
      for (const lk of lookupKeys) {
        const arr = m.get(lk) || [];
        arr.push(Number(id));
        m.set(lk, arr);
      }
      k.set(Number(id), entry.value);
    }
    bindingsByKey = m;
    keyByButtonId = k;
  } catch (_) {
    bindingsByKey = new Map();
    keyByButtonId = new Map();
  }
}

function shouldSkip(ev) {
  if (ev.repeat) return true;
  if (isCaptureActive()) return true;
  const t = ev.target;
  if (!t) return false;
  const tag = t.tagName;
  if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return true;
  if (t.isContentEditable) return true;
  return false;
}

function onKeyDown(ev) {
  if (shouldSkip(ev)) return;
  const raw = resolveKeyName(ev);
  if (!raw) return;
  const name = raw.toLowerCase();
  const prev = pendingKeyDowns.get(name);
  if (prev) clearTimeout(prev);
  const t = setTimeout(() => {
    pendingKeyDowns.delete(name);
    // 30ms window check — any synth-bound 'button-down' arrived?
    const cutoff = performance.now() - (DEFER_MS + 5);
    const ids = bindingsByKey.get(name) || [];
    const isSynth = ids.some(id => {
      const ts = recentButtonDowns.get(id);
      return ts !== undefined && ts >= cutoff;
    });
    if (isSynth) return;
    const svg = currentSvg();
    if (!svg) return;
    activeKeys.add(name);
    for (const id of ids) controller.flashPress(svg, id);
  }, DEFER_MS);
  pendingKeyDowns.set(name, t);
}

function onKeyUp(ev) {
  const raw = resolveKeyName(ev);
  if (!raw) return;
  const name = raw.toLowerCase();
  // Cancel any pending mirror if the key was released before the defer fired.
  const pending = pendingKeyDowns.get(name);
  if (pending) { clearTimeout(pending); pendingKeyDowns.delete(name); }
  if (!activeKeys.has(name)) return;
  activeKeys.delete(name);
  const ids = bindingsByKey.get(name) || [];
  const svg = currentSvg();
  if (!svg) return;
  for (const id of ids) controller.clearPress(svg, id);
}
