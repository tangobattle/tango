export function getPalette(dv: DataView, offset: number): Uint32Array {
  const raw = new Uint16Array(dv.buffer, offset, 16);

  const palette = new Uint32Array(raw.length);
  for (let i = 0; i < raw.length; ++i) {
    const c = raw[i];
    const r = ((c & 0b11111) * 0xff) / 0b11111;
    const g = (((c >> 5) & 0b11111) * 0xff) / 0b11111;
    const b = (((c >> 10) & 0b11111) * 0xff) / 0b11111;
    const a = 0xff;
    // endianness more like smendianness lmao
    palette[i] = (a << 24) | (b << 16) | (g << 8) | r;
  }
  return palette;
}

export function getText(
  dv: DataView,
  scriptOffset: number,
  id: number
): number[] {
  let offset = scriptOffset + dv.getUint16(scriptOffset + id * 0x2, true);
  const nextOffset =
    scriptOffset + dv.getUint16(scriptOffset + (id + 1) * 0x2, true);

  const buf: number[] = [];
  // eslint-disable-next-line no-constant-condition
  while (true) {
    const c = dv.getUint8(offset++);
    if (c == 0xe6 || offset >= nextOffset) {
      break;
    }
    buf.push(c);
  }

  return buf;
}

export function getChipText(
  dv: DataView,
  scriptPointerOffset: number,
  id: number
): number[] {
  if (id > 0xff) {
    scriptPointerOffset += 4;
    id -= 0x100;
  }
  return getText(dv, dv.getUint32(scriptPointerOffset, true) & ~0x08000000, id);
}
