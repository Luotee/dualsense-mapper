// Guided stick capture (spec 5.5), pure: positions in, a calibration out.
// Step 1: both sticks at rest for 3 s (centre = mean; a move restarts the
// clock). Step 2: rotate until the 8 sectors of both sticks are lit (extents
// = min/max per axis). Step 3: result + the reject rules (Rust re-checks).

export const REST_MS = 3000, REST_TOL = 0.15, SECTOR_MAG = 0.8;
export const MAX_CENTER = 0.25, EXTENT = [0.4, 1.25];

function check(cal) {
  for (const [stick, sc] of [['左搖桿', cal.left], ['右搖桿', cal.right]]) {
    for (const [axis, a] of [['X', sc.x], ['Y', sc.y]]) {
      if (!(Math.abs(a.center) <= MAX_CENTER)) return `${stick} ${axis}：中心偏太多（${a.center.toFixed(2)}）`;
      for (const e of [a.neg, a.pos]) if (!(e >= EXTENT[0] && e <= EXTENT[1])) return `${stick} ${axis}：推不到底或範圍不對（${e.toFixed(2)}）`;
    }
  }
  return '';
}

export function createCapture(pad) {
  const c = {
    pad, step: 0, reason: '', ok: false, error: '', result: null,
    t0: 0, sum: [0, 0, 0, 0], n: 0, center: [0, 0, 0, 0], min: [0, 0, 0, 0], max: [0, 0, 0, 0],
    sectors: [Array(8).fill(false), Array(8).fill(false)],
    start(now) {
      Object.assign(c, { step: 1, reason: '', ok: false, error: '', result: null, t0: now, sum: [0, 0, 0, 0], n: 0,
        sectors: [Array(8).fill(false), Array(8).fill(false)] });
    },
    cancel(reason = '') { c.step = 0; c.reason = reason; },
    restLeft(now) { return Math.max(0, Math.ceil((REST_MS - (now - c.t0)) / 1000)); },
    feed(pos, now) {
      if (c.step === 0 || c.step === 3) return;
      if (!pos || pos.pad !== c.pad) { c.cancel('pad'); return; }
      const r = pos.raw;
      if (c.step === 1) {
        const moved = c.n > 0 && r.some((v, i) => Math.abs(v - c.sum[i] / c.n) > REST_TOL);
        if (moved) { c.t0 = now; c.sum = [0, 0, 0, 0]; c.n = 0; }
        r.forEach((v, i) => { c.sum[i] += v; });
        c.n++;
        if (now - c.t0 >= REST_MS) {
          c.center = c.sum.map(s => s / c.n);
          c.min = c.center.slice();
          c.max = c.center.slice();
          c.step = 2;
        }
        return;
      }
      r.forEach((v, i) => { c.min[i] = Math.min(c.min[i], v); c.max[i] = Math.max(c.max[i], v); });
      for (const s of [0, 1]) {
        const dx = r[2 * s] - c.center[2 * s], dy = r[2 * s + 1] - c.center[2 * s + 1];
        if (Math.hypot(dx, dy) > SECTOR_MAG) {
          const deg = (Math.atan2(dy, dx) * 180 / Math.PI + 360) % 360;
          c.sectors[s][Math.round(deg / 45) % 8] = true;
        }
      }
      if (c.sectors.every(s => s.every(Boolean))) {
        const ax = i => ({ center: c.center[i], neg: c.center[i] - c.min[i], pos: c.max[i] - c.center[i] });
        c.result = { left: { x: ax(0), y: ax(1) }, right: { x: ax(2), y: ax(3) } };
        c.error = check(c.result);
        c.ok = !c.error;
        c.step = 3;
      }
    },
  };
  return c;
}
