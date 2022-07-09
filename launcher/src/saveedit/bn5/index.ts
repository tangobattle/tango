export interface GameInfo {
  region: "US" | "JP";
  version: "protoman" | "colonel";
}

const SRAM_START_OFFSET = 0x0100;
const SRAM_SIZE = 0x7c14;
const MASK_OFFSET = 0x1a34;
const GAME_NAME_OFFSET = 0x29e0;
const CHECKSUM_OFFSET = 0x29dc;

function maskSave(dv: DataView) {
  const mask = dv.getUint32(MASK_OFFSET, true);
  const unmasked = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
  for (let i = 0; i < unmasked.length; ++i) {
    unmasked[i] = (unmasked[i] ^ mask) & 0xff;
  }
  // Write the mask back.
  dv.setUint32(MASK_OFFSET, mask, true);
}

function getChecksum(dv: DataView) {
  return dv.getUint32(CHECKSUM_OFFSET, true);
}

function computeChecksum(dv: DataView, version: string) {
  let checksum = CHECKSUM_START[version];
  const arr = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
  for (let i = 0; i < arr.length; ++i) {
    if (i == CHECKSUM_OFFSET) {
      // Don't include the checksum itself in the checksum.
      i += 3;
      continue;
    }
    checksum += arr[i];
  }
  return checksum;
}

const CHECKSUM_START: { [key: string]: number } = {
  protoman: 0x72,
  colonel: 0x18,
};

const GAME_INFOS: { [key: string]: GameInfo } = {
  // Japan
  ROCKEXE5_TOBBRBJ: {
    region: "JP",
    version: "protoman",
  },
  ROCKEXE5_TOCBRKJ: {
    region: "JP",
    version: "colonel",
  },

  // US
  MEGAMAN5_TP_BRBE: {
    region: "US",
    version: "protoman",
  },
  MEGAMAN5_TC_BRKE: {
    region: "US",
    version: "colonel",
  },
};

const ROM_NAMES_BY_SAVE_GAME_NAME: { [key: string]: string } = {
  "REXE5TOB 20041006 US": "MEGAMAN5_TP_BRBE",
  "REXE5TOK 20041006 US": "MEGAMAN5_TC_BRKE",
  "REXE5TOB 20041104 JP": "ROCKEXE5_TOBBRBJ",
  "REXE5TOK 20041104 JP": "ROCKEXE5_TOCBRKJ",
};

export class Editor {
  dv: DataView;
  private romName: string;

  constructor(buffer: ArrayBuffer, romName: string) {
    this.dv = new DataView(buffer);
    this.romName = romName;
  }

  static sramDumpToRaw(buffer: ArrayBuffer) {
    buffer = buffer.slice(SRAM_START_OFFSET, SRAM_START_OFFSET + SRAM_SIZE);
    maskSave(new DataView(buffer));
    return buffer;
  }

  static rawToSRAMDump(buffer: ArrayBuffer) {
    const arr = new Uint8Array(0x10000);
    arr.set(new Uint8Array(buffer), SRAM_START_OFFSET);
    maskSave(new DataView(arr.buffer, SRAM_START_OFFSET, SRAM_SIZE));
    return arr.buffer;
  }

  getROMName() {
    return this.romName;
  }

  static sniff(buffer: ArrayBuffer) {
    if (buffer.byteLength != SRAM_SIZE) {
      throw (
        "invalid byte length of save file: expected " +
        SRAM_SIZE +
        " but got " +
        buffer.byteLength
      );
    }

    buffer = buffer.slice(0);

    const dv = new DataView(buffer);

    const decoder = new TextDecoder("ascii");
    const gn = decoder.decode(
      new Uint8Array(dv.buffer, dv.byteOffset + GAME_NAME_OFFSET, 20)
    );
    if (
      !Object.prototype.hasOwnProperty.call(ROM_NAMES_BY_SAVE_GAME_NAME, gn)
    ) {
      throw "unknown game name: " + gn;
    }

    if (
      getChecksum(dv) !=
      computeChecksum(dv, GAME_INFOS[ROM_NAMES_BY_SAVE_GAME_NAME[gn]].version)
    ) {
      throw "checksum mismatch";
    }

    return [ROM_NAMES_BY_SAVE_GAME_NAME[gn]];
  }

  computeChecksum() {
    return computeChecksum(this.dv, this.getGameInfo().version);
  }

  getCurrentNavi() {
    return this.dv.getUint8(0x2941);
  }

  setCurrentNavi(i: number) {
    this.dv.setUint8(0x2941, i);
  }

  rebuild() {
    this.rebuildChecksum();
  }

  getChecksum() {
    return getChecksum(this.dv);
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  getGameInfo() {
    return GAME_INFOS[this.romName];
  }

  getFolderEditor() {
    return null;
  }

  getNavicustEditor() {
    return null;
  }

  getModcardsEditor() {
    return null;
  }
}
