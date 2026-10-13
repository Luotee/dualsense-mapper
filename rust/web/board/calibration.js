// 設定 → 搖桿: live sticks and the guided calibration (spec 5.5), ported from
// the mockup's stickSVG / calBarHTML. Positions come from get_stick_positions,
// polled at ~30 Hz only while the tab is open.

import { invoke } from '../ipc.js';
import { state } from './store.js';
import { hook } from './events.js';
import { setStickExtras, onSettingsTab, onSettingsSave } from './settings.js';
import { createCapture } from './calib.js';

const R0 = 46;
let latest = null;            // last StickPositions (or null)
let capture = null;           // createCapture(pad) while calibrating
let note = '';                // one-line message under the bar
let timer = null;
const trails = [[], []];

function stickSVG(i, d) {
  const p = latest && latest.pad === state.pad ? latest.cal : [0, 0, 0, 0];
  const x = p[2 * i], y = -p[2 * i + 1];   // stick y is up-positive; SVG y grows down
  const step = capture ? capture.step : 0, dz = d.deadzone * R0;
  let h = '<svg width="112" height="112" viewBox="-56 -56 112 112" class="stk" role="img" aria-label="' + (i ? '右' : '左') + '搖桿位置">';
  h += '<circle r="' + R0 + '" fill="#f6f7fa" stroke="#454c60" stroke-width="2.5"/>';
  if (step >= 2) {
    for (let k = 0; k < 8; k++) {
      const a0 = (k * 45 - 22.5) * Math.PI / 180, a1 = (k * 45 + 22.5) * Math.PI / 180, on = capture.sectors[i][k];
      // sector k is measured with y up; draw it mirrored (SVG y down)
      h += '<path d="M0,0 L' + (R0 * Math.cos(a0)).toFixed(1) + ',' + (-R0 * Math.sin(a0)).toFixed(1) + ' A' + R0 + ',' + R0 + ' 0 0 0 ' +
        (R0 * Math.cos(a1)).toFixed(1) + ',' + (-R0 * Math.sin(a1)).toFixed(1) + ' Z" fill="' + (on ? 'rgba(88,194,58,.45)' : 'rgba(0,0,0,.04)') + '" stroke="#c3c8d2" stroke-width="1"/>';
    }
  }
  h += '<circle r="' + dz.toFixed(1) + '" fill="rgba(168,88,122,.22)" stroke="#a8587a" stroke-width="1" stroke-dasharray="3 2"/>';
  // The crosshair would split the axis sectors in two, so it only shows outside the sector step.
  if (step < 2) h += '<path class="xhair" d="M-' + R0 + ',0H' + R0 + 'M0,-' + R0 + 'V' + R0 + '" stroke="#c3c8d2" stroke-width="1"/>';
  const tr = trails[i];
  if (tr.length > 1) h += '<polyline points="' + tr.map(q => (q[0] * R0).toFixed(1) + ',' + (q[1] * R0).toFixed(1)).join(' ') + '" fill="none" stroke="#4b5a7c" stroke-width="2" stroke-opacity=".35"/>';
  h += '<circle class="sdot" cx="' + (x * R0).toFixed(1) + '" cy="' + (y * R0).toFixed(1) + '" r="6" fill="#4b5a7c" stroke="#2d3142" stroke-width="2"/>';
  return h + '</svg>';
}

function sticksHTML(d) {
  const p = latest && latest.pad === state.pad ? latest.cal : [0, 0, 0, 0];
  let h = '<div class="calrow">';
  for (const i of [0, 1]) {
    const x = p[2 * i], y = p[2 * i + 1], f = v => (v >= 0 ? '+' : '') + v.toFixed(2);
    h += '<figure class="calfig">' + stickSVG(i, d) + '<figcaption>' + (i ? '右搖桿' : '左搖桿') + '<span>' + f(x) + '　' + f(y) + '</span></figcaption></figure>';
  }
  return h + '</div>';
}

function streaming() { return !!latest && latest.pad === state.pad; }

function barHTML() {
  const now = performance.now(), step = capture ? capture.step : 0;
  let msg;
  if (step === 0) msg = note || '搖桿用手把內建的出廠校正。覺得有偏移或推不到底時再校正。';
  else if (step === 1) msg = '1／3　放開兩支搖桿，不要碰 ' + capture.restLeft(now) + ' 秒';
  else if (step === 2) msg = '2／3　把兩支搖桿各轉幾圈，直到八個方向都變綠';
  else if (capture.ok) {
    const off = s => Math.hypot(capture.result[s].x.center, capture.result[s].y.center).toFixed(2);
    msg = '3／3　完成：中心偏移 左 ' + off('left') + '、右 ' + off('right') + '。按「儲存」才會套用。';
  } else msg = '3／3　校正失敗：' + capture.error;
  let h = '<div class="calbar"><span class="calstep">' + msg + (step === 0 && !streaming() ? '　先連上這支手把' : '') + '</span><span class="sp"></span>';
  if (step === 0) {
    h += '<button type="button" class="mini go" data-act="calgo"' + (streaming() ? '' : ' disabled') + '>開始校正搖桿</button>' +
      '<button type="button" class="mini" data-act="calfac">恢復出廠校正</button>';
  } else {
    h += '<button type="button" class="mini" data-act="calstop">' + (step === 3 ? '重來' : '取消') + '</button>';
  }
  return h + '</div>';
}

function paint() {
  const d = state.draft;
  if (!d || state.setTab !== 'stick') return;
  const k = document.querySelector('#board #stkbox'), b = document.querySelector('#board #calbar');
  if (k) k.innerHTML = sticksHTML(d);
  if (b) b.innerHTML = barHTML();
}

async function tick() {
  try { latest = await invoke('get_stick_positions'); } catch (e) { latest = null; }
  const p = latest && latest.pad === state.pad ? latest.cal : null;
  for (const i of [0, 1]) {
    if (p) { trails[i].push([p[2 * i], -p[2 * i + 1]]); if (trails[i].length > 14) trails[i].shift(); } else trails[i].length = 0;
  }
  if (capture && (capture.step === 1 || capture.step === 2)) {
    capture.feed(latest, performance.now());
    if (capture.step === 0 && capture.reason === 'pad') { note = '手把換了，校正已取消'; capture = null; }
  }
  paint();
}

function setPolling(on) {
  if (on && !timer) { timer = setInterval(tick, 33); tick(); }
  if (!on && timer) { clearInterval(timer); timer = null; }
}

export function startCalibration() {
  setStickExtras(d => sticksHTML(d), () => barHTML());
  onSettingsTab((tab, open) => {
    setPolling(open && tab === 'stick');
    if (!open) { capture = null; note = ''; }
  });
  onSettingsSave(async () => {
    if (capture && capture.step === 3 && capture.ok) {
      await invoke('save_stick_cal', { pad: capture.pad, cal: capture.result });
      capture = null;
    }
  });
  hook('click', ev => {
    const el = ev.target.closest('[data-act]');
    if (!state.setOpen || !el) return false;
    const a = el.dataset.act;
    if (a === 'calgo') { if (!streaming()) return true; note = ''; capture = createCapture(state.pad); capture.start(performance.now()); paint(); return true; }
    if (a === 'calstop') { capture = null; note = ''; paint(); return true; }
    if (a === 'calfac') {
      capture = null;
      invoke('reset_stick_cal', { pad: state.pad })
        .then(() => { note = '已恢復出廠校正'; paint(); })
        .catch(e => { note = String(e); paint(); });
      return true;
    }
    return false;
  });
}
