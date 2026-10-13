// ico.mjs — pack PNG images into a .ico (ICONDIR + one ICONDIRENTRY per PNG).
// PNG-compressed entries are valid ICO on Windows Vista and later.

export function packIco(entries) {
  const head = 6 + 16 * entries.length;
  const out = new Uint8Array(head + entries.reduce((n, e) => n + e.png.length, 0));
  const v = new DataView(out.buffer);
  v.setUint16(2, 1, true);
  v.setUint16(4, entries.length, true);
  let off = head;
  entries.forEach((e, i) => {
    const p = 6 + 16 * i, px = e.size >= 256 ? 0 : e.size;
    out[p] = px; out[p + 1] = px;
    v.setUint16(p + 4, 1, true);
    v.setUint16(p + 6, 32, true);
    v.setUint32(p + 8, e.png.length, true);
    v.setUint32(p + 12, off, true);
    out.set(e.png, off);
    off += e.png.length;
  });
  return out;
}
