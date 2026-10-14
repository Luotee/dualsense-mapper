// ViGEmBus prompt for the Switch 2 Pro's Xbox output (spec v3.2.0 §3,
// mockup output-mode card B wording). Display + IPC only: Rust probes the
// bus, downloads, verifies and runs the official installer (iron rule 9).

import { invoke } from '../ipc.js';
import { state, rerender } from './store.js';
import { hook } from './events.js';
import { outputMode } from './view.js';
import * as act from './actions.js';

export const VG = { status: null, probing: false, pop: false, busy: false, err: '' };

const xbox = () => state.pad === 'ns2_pro' && outputMode(state, 'ns2_pro') === 'pad';

// Ask Rust once per change into Xbox mode; `missing` opens the popover.
export async function probe() {
  if (VG.probing) return;
  VG.probing = true;
  try { VG.status = await invoke('get_vigem_status'); } catch (e) { VG.status = 'missing'; }
  VG.probing = false;
  VG.pop = VG.status === 'missing';   // shown only while xbox() (config may still be reloading)
  VG.err = '';
  rerender();
}

// Header extras: the gold ! (popover closed, ViGEmBus missing or the live
// `vigem_missing` warning) and the popover itself.
export function bangHTML() {
  if (!xbox() || VG.pop) return '';
  const live = (state.warnings || []).includes('vigem_missing');
  if (VG.status !== 'missing' && !live) return '';
  return '<button type="button" class="vgbang" data-act="vgopen" aria-label="需要 ViGEmBus" title="需要 ViGEmBus">!</button>';
}

export function popHTML() {
  if (!xbox() || !VG.pop) return '';
  return '<div class="vgpop" role="dialog" aria-label="需要 ViGEmBus"><div><b>這台還沒有 ViGEmBus</b>：要讓遊戲把 Switch 2 Pro 當成 Xbox 手把，需要先安裝它' +
    '（官方安裝程式，約 1 分鐘，會要求系統管理員權限）。沒裝之前，這支手把不會送出任何東西。</div>' +
    (VG.err ? '<div class="vgerr">' + VG.err + '</div>' : '') +
    '<div class="vgbtns"><button type="button" class="mini" data-act="vglater"' + (VG.busy ? ' disabled' : '') + '>稍後</button>' +
    '<button type="button" class="mini" data-act="vgkbm"' + (VG.busy ? ' disabled' : '') + '>改回鍵鼠</button>' +
    '<button type="button" class="mini go" data-act="vginstall"' + (VG.busy ? ' disabled' : '') + '>' + (VG.busy ? '安裝中…' : '安裝 ViGEmBus') + '</button></div></div>';
}

// The banner under the header is hidden while ViGEmBus is known missing:
// the popover / ! says what is wrong instead.
export function vigemBlocksBanner() {
  return xbox() && (VG.status === 'missing' || (state.warnings || []).includes('vigem_missing'));
}

async function install() {
  VG.busy = true; VG.err = ''; rerender();
  try {
    const s = await invoke('install_vigem');
    VG.status = s;
    if (s === 'installed') VG.pop = false;
    else VG.err = '還沒裝好，可以再試一次（狀態：' + s + '）';
  } catch (e) {
    VG.err = '安裝失敗：' + String(e && e.message ? e.message : e);
  }
  VG.busy = false;
  rerender();
}

export function startVigem() {
  // A pick of Xbox re-probes; so does opening the board on the NS2 in Xbox
  // mode (first render with no answer yet).
  act.onModeChange((pad, mode) => { if (pad === 'ns2_pro' && mode === 'pad') { VG.status = null; probe(); } else VG.pop = false; });
  hook('click', ev => {
    const el = ev.target.closest('[data-act]');
    if (!el) return false;
    const a = el.dataset.act;
    if (a === 'vgopen') { VG.pop = true; VG.err = ''; rerender(); return true; }
    if (a === 'vglater') { VG.pop = false; rerender(); return true; }
    if (a === 'vgkbm') { VG.pop = false; act.setMode('kbm'); return true; }
    if (a === 'vginstall') { if (!VG.busy) install(); return true; }
    return false;
  });
}

// Called on every paint: the board opened on the NS2 in Xbox mode.
export function maybeProbe() {
  if (xbox() && VG.status === null && !VG.probing) probe();
}
