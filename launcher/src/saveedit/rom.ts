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

export type ControlCodeHandlers<T> = {
  [code: number]: (
    dv: DataView,
    offset: number
  ) => {
    offset: number;
    value: T;
  } | null;
};

export type NewlineControl = {
  c: "newline";
};

export type ParseOne<T> = (dv: DataView, offset: number) => ParseOneResult<T>;

export type ParseOneResult<T> = {
  offset: number;
  value: { t: number } | T;
} | null;

export function getTextSimple<T>(
  dv: DataView,
  scriptOffset: number,
  id: number,
  charset: string,
  parseOne: ParseOne<NewlineControl | T>
): string {
  return replacePrivateUseCharacters(
    parseText(dv, scriptOffset, id, parseOne)
      .flatMap((chunk) =>
        "t" in chunk
          ? charset[chunk.t]
          : "c" in chunk && chunk.c == "newline"
          ? ["\n"]
          : []
      )
      .join("")
      .replace(/-\n/g, "-")
      .replace(/\n/g, " ")
  );
}

export function parseText<T>(
  dv: DataView,
  scriptOffset: number,
  id: number,
  parseOne: ParseOne<T>
): Array<{ t: number } | T> {
  const chunks: Array<{ t: number } | T> = [];

  let offset = scriptOffset + dv.getUint16(scriptOffset + id * 0x2, true);
  const nextOffset =
    scriptOffset + dv.getUint16(scriptOffset + (id + 1) * 0x2, true);

  while (offset < dv.byteLength && offset < nextOffset) {
    const r = parseOne(dv, offset);
    if (r == null) {
      break;
    }
    const { offset: offset2, value } = r;
    chunks.push(value);
    offset = offset2;
  }

  return chunks;
}

export function getChipText<T>(
  dv: DataView,
  scriptPointerOffset: number,
  id: number,
  charset: string,
  parseOne: ParseOne<NewlineControl | T>
): string {
  if (id > 0xff) {
    scriptPointerOffset += 4;
    id -= 0x100;
  }

  return getTextSimple(
    dv,
    dv.getUint32(scriptPointerOffset, true) & ~0x08000000,
    id,
    charset,
    parseOne
  );
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

export function replacePrivateUseCharacters(s: string) {
  return s.replace(/[\u3000-\uf8ff]/g, (c) => {
    switch (c) {
      case "\ue000":
        return "V2";
      case "\ue001":
        return "V3";
      case "\ue002":
        return "V4";
      case "\ue003":
        return "V5";
      case "\ue004":
        return "EX";
      case "\ue005":
        return "SP";
      case "\ue006":
        return "DS";
      case "\ue007":
        return "RV";
      case "\ue008":
        return "BX";
      case "\ue009":
        return "FZ";
    }
    return c;
  });
}
