import test from 'node:test';
import assert from 'node:assert/strict';
import { packIco } from '../ico.mjs';

const png = n => Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, ...new Array(n).fill(7)]);

test('packIco writes ICONDIR + entries + PNG payloads', () => {
  const a = png(4), b = png(10);
  const ico = packIco([{ size: 16, png: a }, { size: 256, png: b }]);
  const v = new DataView(ico.buffer, ico.byteOffset, ico.byteLength);
  assert.deepEqual([...ico.subarray(0, 6)], [0, 0, 1, 0, 2, 0]);
  assert.equal(ico[6], 16);
  assert.equal(ico[6 + 16], 0, '256 is stored as 0');
  for (let i = 0; i < 2; i++) {
    const e = 6 + 16 * i;
    assert.equal(v.getUint16(e + 4, true), 1, 'planes');
    assert.equal(v.getUint16(e + 6, true), 32, 'bpp');
    const len = v.getUint32(e + 8, true), off = v.getUint32(e + 12, true);
    assert.equal(len, [a, b][i].length);
    assert.deepEqual([...ico.subarray(off, off + 4)], [0x89, 0x50, 0x4e, 0x47]);
  }
  assert.equal(ico.length, 6 + 16 * 2 + a.length + b.length);
});
