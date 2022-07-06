// From https://github.com/SMWCentral/OnlineTools, licensed under the MIT license.
export default function applyBPS(
  rom: Uint8Array,
  patch: Uint8Array,
  ignoreCRC32 = false
) {
  function crc32(bytes: Uint8Array) {
    let c;
    const crcTable: number[] = [];
    for (let n = 0; n < 256; n++) {
      c = n;
      for (let k = 0; k < 8; k++) {
        c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      }
      crcTable[n] = c;
    }

    let crc = 0 ^ -1;
    for (let i = 0; i < bytes.length; i++) {
      crc = (crc >>> 8) ^ crcTable[(crc ^ bytes[i]) & 0xff];
    }
    return (crc ^ -1) >>> 0;
  }

  let patchpos = 0;
  function u8() {
    return patch[patchpos++];
  }
  function u32at(pos: number) {
    return (
      ((patch[pos + 0] << 0) |
        (patch[pos + 1] << 8) |
        (patch[pos + 2] << 16) |
        (patch[pos + 3] << 24)) >>>
      0
    );
  }

  function decode() {
    let ret = 0;
    let sh = 0;
    // eslint-disable-next-line no-constant-condition
    while (true) {
      const next = u8();
      ret += (next ^ 0x80) << sh;
      if (next & 0x80) return ret;
      sh += 7;
    }
  }

  function decodes() {
    const enc = decode();
    let ret = enc >> 1;
    if (enc & 1) ret = -ret;
    return ret;
  }

  if (u8() != 0x42 || u8() != 0x50 || u8() != 0x53 || u8() != 0x31) {
    throw new Error("not a BPS patch");
  }
  const expectedLength = decode();
  if (expectedLength != rom.length)
    throw new Error(
      `wrong input file: length ${expectedLength} != ${rom.length}`
    );

  const actualCRC32 = crc32(rom);
  const expectedCRC32 = u32at(patch.length - 12);
  if (!ignoreCRC32 && actualCRC32 != expectedCRC32) {
    throw new Error(
      `wrong input file: CRC32 ${expectedCRC32
        .toString(16)
        .padStart(8, "0")} != ${actualCRC32.toString(16).padStart(8, "0")}`
    );
  }

  const out = new Uint8Array(decode());
  let outpos = 0;

  const metalen = decode();
  patchpos += metalen; // can't join these two, JS reads patchpos before calling decode

  let inreadpos = 0;
  let outreadpos = 0;

  while (patchpos < patch.length - 12) {
    const thisinstr = decode();
    const len = (thisinstr >> 2) + 1;
    const action = thisinstr & 3;

    switch (action) {
      case 0: // source read
        {
          for (let i = 0; i < len; i++) {
            out[outpos] = rom[outpos];
            outpos++;
          }
        }
        break;
      case 1: // target read
        {
          for (let i = 0; i < len; i++) {
            out[outpos++] = u8();
          }
        }
        break;
      case 2: // source copy
        {
          inreadpos += decodes();
          for (let i = 0; i < len; i++) out[outpos++] = rom[inreadpos++];
        }
        break;
      case 3: // target copy
        {
          outreadpos += decodes();
          for (let i = 0; i < len; i++) out[outpos++] = out[outreadpos++];
        }
        break;
    }
  }

  return out;
}
