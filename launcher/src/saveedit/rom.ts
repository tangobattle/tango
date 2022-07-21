import { getROMInfo } from "../rom";

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
    let c = dv.getUint8(offset++);
    if (c == 0xe6 || offset >= nextOffset) {
      break;
    }
    if (c == 0xe4) {
      c += dv.getUint8(offset++);
    }
    if (c == 0xe5) {
      // Only the Chinese patch does this?
      const hi = dv.getUint8(offset++);
      const lo = dv.getUint8(offset++);
      c = 0xe4 + 0x100 + ((hi << 8) | lo);
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

export function getChipIcon(
  dv: DataView,
  palette: Uint32Array,
  chipIconOffset: number
) {
  const pixels = new Uint32Array(16 * 16);
  const tileBytes = (8 * 8) / 2;

  for (let tileI = 0; tileI < 4; ++tileI) {
    const offset = chipIconOffset + tileBytes * tileI;
    const tile = new Uint8Array(dv.buffer, offset, tileBytes);

    const tileX = tileI % 2;
    const tileY = Math.floor(tileI / 2);

    for (let i = 0; i < tile.length * 2; ++i) {
      const subI = Math.floor(i / 2);
      const paletteIndex = i % 2 == 0 ? tile[subI] & 0xf : tile[subI] >> 4;

      const x = tileX * 8 + (i % 8);
      const y = tileY * 8 + Math.floor(i / 8);

      pixels[y * 16 + x] = palette[paletteIndex];
    }
  }

  return new ImageData(new Uint8ClampedArray(pixels.buffer), 16, 16);
}

export abstract class ROMViewerBase {
  protected dv: DataView;

  constructor(buffer: ArrayBuffer) {
    this.dv = new DataView(buffer);
  }

  public getROMInfo() {
    return getROMInfo(this.dv.buffer);
  }
}
