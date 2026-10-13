import test from 'node:test';
import assert from 'node:assert/strict';
import { createHistory } from '../board/history.js';

const T = n => ({ buttons: { '0': { label: '', type: 'key', value: String(n) } }, quick_l: {}, quick_r: {}, trigger_mode: { l: 'page', r: 'page' } });

test('history_undo_redo_cycle', () => {
  const now = { dualsense: T(2), ns_pro: T(9) };
  const h = createHistory();
  assert.equal(h.canUndo, false);
  h.record('dualsense', T(1));
  assert.equal(h.canUndo, true);
  assert.deepEqual(h.undo(p => now[p]), { pad: 'dualsense', tables: T(1) });
  assert.equal(h.canUndo, false);
  assert.equal(h.canRedo, true);
  now.dualsense = T(1);
  assert.deepEqual(h.redo(p => now[p]), { pad: 'dualsense', tables: T(2) });
  assert.equal(h.canRedo, false);
});

test('history_redo_returns_the_state_before_undo', () => {
  const now = { dualsense: T(2) };
  const h = createHistory();
  h.record('dualsense', T(1));
  const u = h.undo(p => now[p]);
  assert.deepEqual(u.tables, T(1));
  now.dualsense = T(1);
  const r = h.redo(p => now[p]);
  assert.deepEqual(r, { pad: 'dualsense', tables: T(2) });
  assert.equal(h.canRedo, false);
  assert.equal(h.canUndo, true);
});

test('history_record_clears_redo', () => {
  const now = { dualsense: T(2) };
  const h = createHistory();
  h.record('dualsense', T(1));
  h.undo(p => now[p]);
  assert.equal(h.canRedo, true);
  h.record('dualsense', T(5));
  assert.equal(h.canRedo, false);
});

test('undo_of_rejected_entry_is_dropped', () => {
  const now = { dualsense: T(3), ns_pro: T(8) };
  const h = createHistory();
  h.record('ns_pro', T(7));
  h.record('dualsense', T(2));
  const u = h.undo(p => now[p]);
  assert.equal(u.pad, 'dualsense');
  h.dropLast('undo');           // the backend refused u: forget it entirely
  assert.equal(h.canRedo, false);
  assert.equal(h.canUndo, true);
  assert.equal(h.undo(p => now[p]).pad, 'ns_pro');
});

test('entries_are_deep_copies', () => {
  const h = createHistory();
  const before = T(1);
  h.record('dualsense', before);
  before.buttons['0'].value = 'mutated';
  assert.equal(h.undo(() => T(2)).tables.buttons['0'].value, '1');
});
