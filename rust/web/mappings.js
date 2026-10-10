import { invoke, listen } from './ipc.js';
import * as controller from './controller.js';
import * as bindPopup from './bind_popup.js';

let config = null;
let svgEl  = null;
let rafQueued = new Map();   // id → frame id, so we don't queue duplicate flashes

export async function init() {
  const root = document.getElementById('pane-mappings');
  await reload();
  svgEl = root.querySelector('svg.controller');
  hookClick();
  hookLiveHighlight();
  listenConfigChanged();
  renderChipList();

  const btn = document.getElementById('btn-disconnect');
  if (btn) {
    btn.addEventListener('click', async () => {
      try {
        await invoke('disconnect_gamepad');
      } catch (e) {
        console.error('disconnect_gamepad failed:', e);
      }
    });
  }
}

async function reload() {
  config = await invoke('get_config');
  const bindings = {};
  for (const [id, entry] of Object.entries(config.buttons)) {
    bindings[id] = { kind: kindOf(entry), value: valueOf(entry) };
  }
  controller.render(document.getElementById('controller-host'), bindings);
  svgEl = document.querySelector('#controller-host svg.controller');
}

// ButtonEntry serialises flattened (rust/src/config.rs ButtonEntry has
// #[serde(flatten)] over Binding; Binding has #[serde(tag = "type",
// content = "value", rename_all = "lowercase")]), so an entry looks like:
//     { label: "Cross", type: "key",     value: "x" }
//     { label: "L2",    type: "macro",   value: "macro_A" }
//     { label: "PS",    type: "unbound"                  }
function kindOf(entry) {
  return entry?.type ?? 'unbound';
}
function valueOf(entry) {
  return entry?.value;
}

function hookClick() {
  document.getElementById('controller-host').addEventListener('click', e => {
    const hit = e.target.closest('[data-id]');
    if (!hit) return;
    const id = Number(hit.dataset.id);
    openPopup(id);
  });
}

function openPopup(id) {
  const entry = config.buttons[String(id)];
  if (!entry) return;
  controller.selectButton(svgEl, id);
  bindPopup.open({
    id,
    label: entry.label,
    currentEntry: entry,
    macros: Object.keys(config.macros || {}),
    onSaved: () => { /* reload happens in onClosed */ },
    onClosed: async () => {
      controller.clearSelection(svgEl);
      // config-changed event from the bridge will refresh; but explicitly reload
      // now in case the save was a no-op or the event hasn't arrived yet.
      await reload();
      renderChipList();
    },
  });
}

function hookLiveHighlight() {
  listen('button-down', payload => {
    const id = payload.id;
    if (rafQueued.has(id)) return;
    const f = requestAnimationFrame(() => {
      controller.flashPress(svgEl, id);
      rafQueued.delete(id);
    });
    rafQueued.set(id, f);
  });
  listen('button-up', payload => {
    controller.clearPress(svgEl, payload.id);
  });
  listen('touchpad-click', payload => {
    controller.showTouchpadDot(svgEl, payload.raw_x, payload.raw_y);
  });
  listen('touchpad-hover', payload => {
    // Sentinel: quadrant=255 means finger lifted.
    if (payload.quadrant === 255) {
      controller.clearTouchpadHover(svgEl);
    } else {
      controller.showTouchpadHover(svgEl, payload.quadrant, payload.raw_x, payload.raw_y);
    }
  });
}

function listenConfigChanged() {
  listen('config-changed', async () => {
    await reload();
    renderChipList();
  });
}

function renderChipList() {
  const host = document.getElementById('chip-list');
  if (!host) return;
  host.innerHTML = '';
  for (let id = 0; id <= 32; id++) {
    const entry = config.buttons[String(id)];
    if (!entry) continue;
    const row = document.createElement('div');
    row.className = 'chip-row';
    row.dataset.id = id;
    const kind = kindOf(entry);
    row.innerHTML = `
      <span class="chip-label">${escape(entry.label)}</span>
      ${renderChip(entry, kind)}
    `;
    row.addEventListener('click', () => openPopup(id));
    host.appendChild(row);
  }
}

function renderChip(entry, kind) {
  if (kind === 'key')   return `<code class="chip chip-key">${escape(valueOf(entry))}</code>`;
  if (kind === 'macro') return `<code class="chip chip-macro">&#x26A1; ${escape(valueOf(entry))}</code>`;
  if (kind === 'mouse') return `<code class="chip chip-mouse">&#x1F5B1; ${escape(valueOf(entry))}</code>`;
  return `<span class="chip-mute">unbound</span>`;
}
function escape(s) { return String(s).replace(/[<>&]/g, c => ({'<': '&lt;', '>': '&gt;', '&': '&amp;'}[c])); }
