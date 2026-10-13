// Which pad's binding table today's screens show, and its base table.
//
// The config has one table per controller (`config.pads[slug]`). Until the
// board (PR 2) adds a pad selector, the Mappings / Macros / keyboard-mirror
// screens show the table of the connected controller, or the DualSense one
// when none is connected. Display choice only: every mapping decision stays
// in Rust (iron rule 9).

export const PAD_SLUGS = ['dualsense', 'ns_pro', 'ns2_pro'];

// `status`: a ControllerStatus (get_controller_status) or a
// controller-status event payload; null / disconnected → 'dualsense'.
export function shownPad(status) {
  const kind = status && status.connected !== false ? status.kind : null;
  return PAD_SLUGS.includes(kind) ? kind : 'dualsense';
}

export function baseButtons(config, pad) {
  return config?.pads?.[pad]?.buttons ?? {};
}

// Every [page, id, entry] of every pad, for macro cross-references.
export function allSlots(config) {
  const out = [];
  for (const pad of PAD_SLUGS) {
    const p = config?.pads?.[pad];
    if (!p) continue;
    for (const page of ['buttons', 'quick_l', 'quick_r']) {
      for (const [id, entry] of Object.entries(p[page] || {})) out.push({ pad, page, id, entry });
    }
  }
  return out;
}

// ─── The shown pad, shared by the screens ────────────────────────────────────

let status = null;
const subscribers = new Set();

export function setStatus(s) {
  const before = shownPad(status);
  status = s;
  if (shownPad(status) !== before) for (const f of subscribers) f();
}

export function currentPad() {
  return shownPad(status);
}

export function onPadChange(f) {
  subscribers.add(f);
}
