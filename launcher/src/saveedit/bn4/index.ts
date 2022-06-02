import CHIPS from "./data/chips.json";

export { CHIPS };

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

export interface GameInfo {
  region: "US" | "JP";
  version: "bluemoon" | "redsun";
}

function getChecksum(dv: DataView) {
  return dv.getUint32(0x21e8, true);
}

function computeChecksum(dv: DataView, gameInfo: GameInfo) {
  return (
    computeChecksumRaw(dv) +
    Editor.CHECKSUM_START[gameInfo.version] +
    (gameInfo.region != "JP" ? new Uint8Array(dv.buffer, 0, 1)[0] : 0)
  );
}

function computeChecksumRaw(dv: DataView) {
  let checksum = 0;
  const arr = new Uint8Array(dv.buffer, 0, dv.buffer.byteLength);
  for (let i = 1; i < dv.buffer.byteLength; ++i) {
    if (i == 0x21e8 + dv.byteOffset) {
      // Don't include the checksum itself in the checksum.
      i += 3;
      continue;
    }
    checksum += arr[i];
  }
  return checksum;
}

export class Editor {
  private dv: DataView;
  private romName: string;

  static SRAM_END_OFFSET = 0x73d2;
  static ROM_NAMES_BY_CHECKSUM_GUESS: { [key: string]: string } = {
    JPbluemoon: "ROCK_EXE4_BMB4BJ",
    JPredsun: "ROCK_EXE4_RSB4WJ",
    USbluemoon: "MEGAMANBN4BMB4BE",
    USredsun: "MEGAMANBN4RSB4WE",
  };

  static GAME_INFOS: { [key: string]: GameInfo } = {
    // Japan
    ROCK_EXE4_BMB4BJ: {
      region: "JP",
      version: "bluemoon",
    },
    ROCK_EXE4_RSB4WJ: {
      region: "JP",
      version: "redsun",
    },

    // US
    MEGAMANBN4BMB4BE: {
      region: "US",
      version: "bluemoon",
    },
    MEGAMANBN4RSB4WE: {
      region: "US",
      version: "redsun",
    },
  };

  static CHECKSUM_START: { [key: string]: number } = {
    bluemoon: 0x22,
    redsun: 0x16,
  };

  getROMName() {
    return this.romName;
  }

  static getStartOffset(buffer: ArrayBuffer) {
    const dv = new DataView(buffer);
    const startOffset = dv.getUint32(0x1550, true);
    if (startOffset > 0x1fc || (startOffset & 3) != 0) {
      return null;
    }
    return startOffset;
  }

  static sramDumpToRaw(buffer: ArrayBuffer) {
    buffer = buffer.slice(0, Editor.SRAM_END_OFFSET);
    const dv = new DataView(buffer);
    Editor.maskSave(dv);
    return buffer;
  }

  static rawToSramDump(buffer: ArrayBuffer) {
    const arr = new Uint8Array(0x10000);
    arr.set(new Uint8Array(buffer));
    Editor.maskSave(new DataView(arr.buffer));
    return arr.buffer;
  }

  static fromUnmaskedSRAM(buffer: ArrayBuffer) {
    if (buffer.byteLength != Editor.SRAM_END_OFFSET) {
      throw (
        "invalid byte length of save file: expected " +
        Editor.SRAM_END_OFFSET +
        " but got " +
        buffer.byteLength
      );
    }

    buffer = buffer.slice(0);

    const startOffset = Editor.getStartOffset(buffer);
    if (startOffset == null) {
      throw "could not locate start offset";
    }
    const dv = new DataView(buffer, startOffset);

    const decoder = new TextDecoder("ascii");
    const gn = decoder.decode(
      new Uint8Array(buffer, dv.byteOffset + 0x2208, 20)
    );
    if (gn != "ROCKMANEXE4 20031022") {
      throw "unknown game name: " + gn;
    }

    const checksum = getChecksum(dv);
    const rawChecksum = computeChecksumRaw(dv);
    const firstVal = new Uint8Array(buffer, 0, 1)[0];

    let version;
    let region;
    switch (checksum) {
      case rawChecksum + Editor.CHECKSUM_START.bluemoon: {
        version = "bluemoon";
        region = "JP";
        break;
      }
      case rawChecksum + Editor.CHECKSUM_START.redsun: {
        version = "redsun";
        region = "JP";
        break;
      }
      case rawChecksum + Editor.CHECKSUM_START.bluemoon + firstVal: {
        version = "bluemoon";
        region = "US";
        break;
      }
      case rawChecksum + Editor.CHECKSUM_START.redsun + firstVal: {
        version = "redsun";
        region = "US";
        break;
      }
      default:
        throw "unknown game, no checksum formats match";
    }

    const checksumGuess = region + version;
    return new Editor(
      buffer,
      Editor.ROM_NAMES_BY_CHECKSUM_GUESS[checksumGuess]
    );
  }

  constructor(buffer: ArrayBuffer, romName: string, verifyChecksum = true) {
    const startOffset = Editor.getStartOffset(buffer);
    if (startOffset == null) {
      throw "could not locate start offset";
    }

    this.dv = new DataView(buffer, startOffset);
    this.romName = romName;

    if (verifyChecksum && this.getChecksum() != this.computeChecksum()) {
      throw "checksum does not match";
    }
  }

  static maskSave(dv: DataView) {
    const mask = dv.getUint32(0x1554, true);
    const unmasked = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
    for (let i = 0; i < unmasked.length; ++i) {
      unmasked[i] = (unmasked[i] ^ mask) & 0xff;
    }
    // Write the mask back.
    dv.setUint32(0x1554, mask, true);
  }

  getGameInfo() {
    return Editor.GAME_INFOS[this.romName];
  }

  getGameFamily() {
    return "bn4";
  }

  getChecksum() {
    return getChecksum(this.dv);
  }

  rebuildChecksum() {
    return this.dv.setUint32(0x21e8, this.computeChecksum(), true);
  }

  computeChecksum() {
    return computeChecksum(this.dv, this.getGameInfo());
  }

  getFolderCount() {
    return 3; // TODO
  }

  getEquippedFolder() {
    return this.dv.getUint8(0x2132);
  }

  setEquippedFolder(i: number) {
    return this.dv.setUint8(0x2132, i);
  }

  getRegularChipIndex(folderIdx: number) {
    // Only the first chip can be the regular chip, but the value here is the insertion order of the regular chip for display.
    const i = this.dv.getUint8(0x214d + folderIdx);
    return i != 0xff ? 0 : null;
  }

  setRegularChipIndex(folderIdx: number, i: number) {
    if (i != 0 && i != null) {
      throw "must be 0 or null";
    }
    this.dv.setUint8(0x214d + folderIdx, i == null ? 0xff : 0);
  }

  getTagChip1Index() {
    // Not supported.
    return null;
  }

  getTagChip2Index() {
    // Not supported.
    return null;
  }

  getChipData() {
    return CHIPS;
  }

  getChipCount(id: number, code: string) {
    return this.getChipCountRaw(id, CHIPS[id]!.codes!.indexOf(code));
  }

  getChipCountRaw(id: number, variant: number) {
    return this.dv.getUint8(0x26e4 + ((id * 0xc) | variant));
  }

  setChipCount(id: number, code: string, n: number) {
    this.setChipCountRaw(id, CHIPS[id]!.codes!.indexOf(code), n);
  }

  setChipCountRaw(id: number, variant: number, n: number) {
    this.dv.setUint8(0x26e4 + ((id * 0xc) | variant), n);
  }

  getRawBufferForSave() {
    if (this.getChecksum() != this.computeChecksum()) {
      throw "checksum does not match";
    }
    return this.dv.buffer;
  }

  getChipRaw(folderIdx: number, chipIdx: number) {
    const chipConstant = this.dv.getUint16(
      0x262c + folderIdx * (30 * 2) + chipIdx * 2,
      true
    );

    if (chipConstant == 0) {
      return null;
    }

    return {
      id: chipConstant & 0x1ff,
      variant: chipConstant >> 9,
    };
  }

  getChip(folderIdx: number, chipIdx: number) {
    const rawChip = this.getChipRaw(folderIdx, chipIdx);
    if (rawChip == null) {
      return null;
    }

    return {
      id: rawChip.id,
      code: CHIP_CODES[rawChip.variant],
    };
  }

  setChipRaw(folderIdx: number, chipIdx: number, id: number, variant: number) {
    this.dv.setUint16(
      0x262c + folderIdx * (30 * 2) + chipIdx * 2,
      id | (variant << 9),
      true
    );
  }

  setChip(folderIdx: number, chipIdx: number, id: number, code: string) {
    this.setChipRaw(folderIdx, chipIdx, id, CHIP_CODES.indexOf(code));
  }

  rebuild() {
    this.rebuildChecksum();
  }
}
