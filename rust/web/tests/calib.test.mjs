import test from 'node:test';
import assert from 'node:assert/strict';
import { createCapture, REST_MS } from '../board/calib.js';

const P = (raw, pad = 'ns2_pro') => ({ pad, raw, cal: raw });

// Feed `ms` of rest at (lx, ly) for both sticks, 20 ms apart, from time t.
function rest(c, t, ms, x, y) {
  for (let k = 0; k <= ms; k += 20) c.feed(P([x, y, x, y]), t + k);
  return t + ms + 20;
}
// One full circle of radius r around (x, y) on both sticks.
function circle(c, t, x, y, r) {
  for (let a = 0; a < 360; a += 5) {
    const dx = r * Math.cos(a * Math.PI / 180), dy = r * Math.sin(a * Math.PI / 180);
    c.feed(P([x + dx, y + dy, x + dx, y + dy]), t);
    t += 10;
  }
  return t;
}

test('rest_then_rotate_produces_cal', () => {
  const c = createCapture('ns2_pro');
  c.start(0);
  assert.equal(c.step, 1);
  let t = rest(c, 0, REST_MS + 100, 0.05, -0.02);
  assert.equal(c.step, 2);
  t = circle(c, t, 0.05, -0.02, 0.9);
  assert.equal(c.step, 3);
  assert.ok(c.ok, c.error);
  const lx = c.result.left.x, ly = c.result.left.y;
  assert.ok(Math.abs(lx.center - 0.05) < 1e-6 && Math.abs(ly.center + 0.02) < 1e-6, JSON.stringify(c.result.left));
  assert.ok(Math.abs(lx.pos - 0.9) < 0.01 && Math.abs(lx.neg - 0.9) < 0.01, JSON.stringify(lx));
  assert.ok(c.sectors.every(s => s.every(Boolean)));
});

test('moving_during_rest_restarts_timer', () => {
  const c = createCapture('ns2_pro');
  c.start(0);
  rest(c, 0, 2000, 0, 0);
  c.feed(P([0.5, 0, 0, 0]), 2100);   // nudged
  rest(c, 2120, 2000, 0, 0);
  assert.equal(c.step, 1, 'only 2 s of rest since the nudge');
  rest(c, 4140, 1200, 0, 0);
  assert.equal(c.step, 2);
});

test('bad_center_is_rejected', () => {
  const c = createCapture('ns2_pro');
  c.start(0);
  let t = rest(c, 0, REST_MS + 100, 0.3, 0);
  t = circle(c, t, 0.3, 0, 0.9);
  assert.equal(c.step, 3);
  assert.equal(c.ok, false);
  assert.match(c.error, /中心/);
});

test('capture_cancels_when_owner_changes', () => {
  const c = createCapture('ns2_pro');
  c.start(0);
  rest(c, 0, 1000, 0, 0);
  c.feed(P([0, 0, 0, 0], 'dualsense'), 1100);
  assert.equal(c.step, 0);
  assert.equal(c.reason, 'pad');
  const d = createCapture('ns2_pro');
  d.start(0);
  d.feed(null, 10);
  assert.equal(d.step, 0);
  assert.equal(d.reason, 'pad');
});

test('extent between polls lights its sector', () => {
  const c = createCapture('ns2_pro');
  c.start(0);
  const t = rest(c, 0, REST_MS + 100, 0, 0);
  assert.equal(c.step, 2);
  // Centred sample, but the stick reached the right rim between two polls.
  c.feed({ pad: 'ns2_pro', raw: [0, 0, 0, 0], cal: [0, 0, 0, 0], raw_min: [0, 0, 0, 0], raw_max: [1.0, 0, 0, 0] }, t);
  assert.equal(c.sectors[0][0], true);
  assert.equal(c.max[0], 1.0);
});

test('extent corners light the diagonal sector', () => {
  const c = createCapture('ns2_pro');
  c.start(0);
  const t = rest(c, 0, REST_MS + 100, 0, 0);
  c.feed({ pad: 'ns2_pro', raw: [0, 0, 0, 0], cal: [0, 0, 0, 0], raw_min: [-0.7, -0.7, 0, 0], raw_max: [0, 0, 0, 0] }, t);
  assert.equal(c.sectors[0][5], true);   // 225°
  assert.equal(c.min[1], -0.7);
});
