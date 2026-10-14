// The board as one HTML string (spec 4.1), ported from the mockup's mockHTML
// for the live screen. Pure: state in, markup out.

import { layoutFor, slotsOf, groupSlots, routeHTML } from './layout.js';
import { padSvg } from './padart.js';
import { esc, tagSvg, TAGTXT } from './tags.js';
import { LABEL } from './keys.js';
import { boardBinds, PADS, PAD_NAMES } from './model.js';
import { keyboardHTML, mouseHTML, macroBoxHTML } from './palette.js';
import { ICON } from './icons.js';
import { editorHTML } from './macros.js';
import { settingsHTML } from './settings.js';

export const CAPTURE_TEXT = '按鍵鼠上的鍵，或點下面的方塊（Esc 取消）';

export function triggerModes(config, pad) {
  return config?.pads?.[pad]?.trigger_mode || { l: 'page', r: 'page' };
}

export function mappingOn(config, pad) {
  const v = config?.mapping_enabled?.[pad];
  return v === undefined ? true : v;
}

export function disp(k) {
  if (!k) return '';
  if (k.startsWith('M:')) return k.slice(2);
  return LABEL[k] || k;
}

function slotBtn(st, key, tag, x, y, b) {
  const k = b[key] || '', mac = k.startsWith('M:'), wait = st.sel === key;
  return '<button type="button" class="slot' + (k ? ' on' : '') + (mac ? ' mac' : '') + (st.held[key] ? ' hit' : '') + (wait ? ' wait sel' : '') +
    '" style="left:' + x + 'px;top:' + y + 'px" data-slot="' + key + '" draggable="' + (k ? 'true' : 'false') + '" aria-label="' +
    esc((TAGTXT[tag] || tag) + (k ? '：' + (mac ? '巨集 ' : '') + disp(k) : '：未設定')) + '"><span class="tag">' + tagSvg(tag) + '</span>' +
    (wait ? '<span class="kc" aria-hidden="true">' + ICON.keycap + '</span>' : '') + '<span class="val">' + esc(disp(k)) + '</span></button>';
}

function qcapHTML(pad, side, mode) {
  const on = mode === 'page', name = pad === 'dualsense' ? side + '2' : 'Z' + side;
  return '<span class="seg">' + name + '<button type="button" data-qs="' + side + '" data-mode="quick" aria-pressed="' + on + '">快捷頁</button>' +
    '<button type="button" data-qs="' + side + '" data-mode="single" aria-pressed="' + !on + '">單鍵</button></span>';
}

function sw(act, label, on) {
  return '<button type="button" class="sw' + (on ? ' on' : '') + '" role="switch" aria-checked="' + on + '" data-act="' + act + '"><span>' + label +
    '</span><span class="track"><span class="knob"></span></span></button>';
}

export function render(st) {
  const pad = st.pad, cfg = st.config, modes = triggerModes(cfg, pad), L = layoutFor(pad, modes), b = boardBinds(cfg, pad);
  let h = '<header class="hdr"><div class="ttl">按鍵設定</div><div class="pads">';
  for (const p of PADS) {
    const c = st.connected.has(p), act = pad === p;
    h += '<span class="pdw' + (act ? ' act' : '') + (c ? ' c' : '') + '"><button type="button" class="pd' + (act ? ' act' : '') + '" data-pad="' + p +
      '" aria-pressed="' + act + '" aria-label="' + PAD_NAMES[p] + (c ? '（已連線）' : '（未連線）') + '"><span class="live' + (c ? '' : ' idle') + '"></span><span>' +
      PAD_NAMES[p] + '</span></button>' +
      (c ? '<button type="button" class="pwr" data-act="cut" data-cutpad="' + p + '" aria-label="斷開 ' + PAD_NAMES[p] + '" title="斷開手把">' + ICON.power + '</button>' : '') + '</span>';
  }
  h += '</div><div class="sp"></div>' + sw('rumble', '連線震動', cfg.rumble_on_connect !== false) + sw('map', '鍵鼠對應', mappingOn(cfg, pad)) +
    '<button type="button" class="gear" data-act="setopen" aria-label="設定" title="設定">' + ICON.gear + '</button></header>';
  h += '<div class="stage"></div>' + padSvg(pad);
  const hot = {};
  for (const g of L.groups) {
    if (g.q) h += '<div class="qcap" style="left:' + (g.x - 124) + 'px;top:' + (g.y - 82) + 'px">' + qcapHTML(pad, g.q, g.q === 'L' ? modes.l : modes.r) + '</div>';
    if (g.kind === 'tpsw') {
      const on = cfg.touchpad_cursor_enabled !== false;
      h += '<button type="button" class="tstack' + (on ? ' on' : '') + '" style="left:' + (g.x - 27) + 'px;top:' + (g.y - 27) + 'px" role="switch" aria-checked="' + on +
        '" aria-label="觸控板移動滑鼠游標" data-act="tp">' + ICON.touchpad + '<span class="track"><span class="knob"></span></span></button>';
    }
    for (const s of groupSlots(g)) {
      h += slotBtn(st, s.key, s.tag, s.x, s.y, b);
      if (st.held[s.key]) hot[g.id] = true;
    }
  }
  // Lines last: always on top (spec 4.3).
  h += '<svg class="lines" viewBox="0 0 980 640" aria-hidden="true">' + L.routes.map(r => routeHTML(r.pts, r.r, r.m, !!hot[r.g])).join('') + '</svg>';
  const lock = st.medit ? ' locked' : '';
  const waiting = st.sel !== null && !st.medit;
  h += '<section class="pal">' + (waiting ? '<div class="capbar" role="status">' + CAPTURE_TEXT + '</div>' : '') + keyboardHTML(st) +
    '<div class="m3s' + lock + '">' + mouseHTML(st) + '</div>' + macroBoxHTML(st, lock) + '</section>';
  if (st.medit) h += editorHTML(st);
  if (st.setOpen) h += settingsHTML(st);
  h += '<footer class="ftr"><button type="button" class="b-def" data-act="reset">恢復預設</button><button type="button" class="b-clr" data-act="clear">全部清除</button>' +
    '<span class="note">' + esc(st.note) + '</span><div class="sp"></div>' +
    '<button type="button" class="b-hist" data-act="undo" title="復原（Ctrl+Z）"' + (st.canUndo ? '' : ' disabled') + '>' + ICON.undo + '復原</button>' +
    '<button type="button" class="b-hist redo" data-act="redo" title="重做（Ctrl+Y）"' + (st.canRedo ? '' : ' disabled') + '><span class="flip">' + ICON.undo + '</span>重做</button></footer>';
  return h;
}

export { slotsOf };
