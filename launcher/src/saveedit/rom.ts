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

export interface ParseTextOptions<Control> {
  controlCodeHandlers: ControlCodeHandlers<Control>;
  extendCharsetControlCode: number;
}

export type ControlCodeHandlers<Control> = {
  [code: number]: (
    dv: DataView,
    offset: number
  ) => {
    offset: number;
    control: Control;
  } | null;
};

export type NewlineControl = {
  c: "newline";
};

export function parseText<Control>(
  dv: DataView,
  scriptOffset: number,
  id: number,
  opts: ParseTextOptions<Control>
): Array<{ t: number[] } | Control> {
  const chunks: Array<{ t: number[] } | Control> = [];

  let offset = scriptOffset + dv.getUint16(scriptOffset + id * 0x2, true);
  const nextOffset =
    scriptOffset + dv.getUint16(scriptOffset + (id + 1) * 0x2, true);

  const text: number[] = [];
  while (offset < dv.byteLength && offset < nextOffset) {
    let c = dv.getUint8(offset++);

    const handler = opts.controlCodeHandlers[c];
    if (handler) {
      chunks.push({ t: text.splice(0, text.length) });
      const r = handler(dv, offset);
      if (r == null) {
        break;
      }
      const { offset: offset2, control } = r;
      chunks.push(control);
      offset = offset2;
      continue;
    }

    if (c == opts.extendCharsetControlCode) {
      c += dv.getUint8(offset++);
    }

    text.push(c);
  }

  if (text.length > 0) {
    chunks.push({ t: text.splice(0, text.length) });
  }

  return chunks;
}

export function getChipText<Control>(
  dv: DataView,
  scriptPointerOffset: number,
  id: number,
  opts: ParseTextOptions<Control | NewlineControl>
): number[] {
  if (id > 0xff) {
    scriptPointerOffset += 4;
    id -= 0x100;
  }

  return parseText(
    dv,
    dv.getUint32(scriptPointerOffset, true) & ~0x08000000,
    id,
    opts
  ).flatMap((chunk) => ("t" in chunk ? chunk.t : []));
}

export function getTiles(
  dv: DataView,
  palette: Uint32Array,
  startOffset: number,
  tileW: number,
  tileH: number
) {
  const pixels = new Uint32Array(8 * tileW * 8 * tileH);
  const tileBytes = (8 * 8) / 2;

  for (let tileI = 0; tileI < tileW * tileH; ++tileI) {
    const offset = startOffset + tileBytes * tileI;
    const tile = new Uint8Array(dv.buffer, offset, tileBytes);

    const tileX = tileI % tileW;
    const tileY = Math.floor(tileI / tileW);

    for (let i = 0; i < tile.length * 2; ++i) {
      const subI = Math.floor(i / 2);
      const paletteIndex = i % 2 == 0 ? tile[subI] & 0xf : tile[subI] >> 4;

      const x = tileX * 8 + (i % 8);
      const y = tileY * 8 + Math.floor(i / 8);

      pixels[y * (tileW * 8) + x] =
        paletteIndex > 0 ? palette[paletteIndex] : 0;
    }
  }

  return new ImageData(
    new Uint8ClampedArray(pixels.buffer),
    tileW * 8,
    tileH * 8
  );
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

export function unlz77(dv: DataView) {
  const out: number[] = [];

  let pos = 0;

  const header = dv.getUint32(pos, true);
  pos += 4;

  if ((header & 0xff) != 0x10) {
    throw "invalid lz77 data";
  }

  const n = header >> 8;
  while (out.length < n) {
    const ref = dv.getUint8(pos);
    pos += 1;

    for (let i = 0; i < 8 && out.length < n; ++i) {
      if ((ref & (0x80 >> i)) == 0) {
        out.push(dv.getUint8(pos));
        pos += 1;
        continue;
      }

      const info = dv.getUint16(pos, false);
      pos += 2;

      const m = info >> 12;
      const offset = info & 0x0fff;

      for (let j = 0; j < m + 3; ++j) {
        out.push(out[out.length - offset - 1]);
      }
    }
  }

  return new Uint8Array(out.slice(0, n)).buffer;
}
