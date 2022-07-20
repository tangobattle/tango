import { ROMViewerBase } from "../rom";
import CHIPS from "./data/chips.json";

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

const SRAM_SIZE = 0x73d2;
const MASK_OFFSET = 0x1554;
const GAME_NAME_OFFSET = 0x2208;
const CHECKSUM_OFFSET = 0x21e8;

const GAME_INFOS: { [key: string]: GameInfo } = {
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

const CHECKSUM_START: { [key: string]: number } = {
  bluemoon: 0x22,
  redsun: 0x16,
};

function maskSave(dv: DataView) {
  const mask = dv.getUint32(MASK_OFFSET, true);
  const unmasked = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
  for (let i = 0; i < unmasked.length; ++i) {
    unmasked[i] = (unmasked[i] ^ mask) & 0xff;
  }
  // Write the mask back.
  dv.setUint32(MASK_OFFSET, mask, true);
}

export interface GameInfo {
  region: "US" | "JP";
  version: "bluemoon" | "redsun";
}

function getChecksum(dv: DataView) {
  return dv.getUint32(CHECKSUM_OFFSET, true);
}

function computeChecksum(dv: DataView, gameInfo: GameInfo) {
  return (
    computeChecksumRaw(dv) +
    CHECKSUM_START[gameInfo.version] +
    (gameInfo.region != "JP" ? new Uint8Array(dv.buffer, 0, 1)[0] : 0)
  );
}

function computeChecksumRaw(dv: DataView) {
  let checksum = 0;
  const arr = new Uint8Array(dv.buffer, 0, dv.buffer.byteLength);
  for (let i = 1; i < dv.buffer.byteLength; ++i) {
    if (i == CHECKSUM_OFFSET + dv.byteOffset) {
      // Don't include the checksum itself in the checksum.
      i += 3;
      continue;
    }
    checksum += arr[i];
  }
  return checksum;
}

class FolderEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getFolderCount() {
    return 3; // TODO
  }

  getEquippedFolder() {
    return this.editor.dv.getUint8(0x2132);
  }

  setEquippedFolder(i: number) {
    return this.editor.dv.setUint8(0x2132, i);
  }

  isRegularChipInPlace() {
    return false;
  }

  getRegularChipIndex(folderIdx: number) {
    const i = this.editor.dv.getUint8(0x214d + folderIdx);
    return i != 0xff ? i : null;
  }

  setRegularChipIndex(folderIdx: number, i: number | null) {
    this.editor.dv.setUint8(0x214d + folderIdx, i == null ? 0xff : i);
  }

  getTagChip1Index() {
    // Not supported.
    return null;
  }

  getTagChip2Index() {
    // Not supported.
    return null;
  }

  getChipInfo(id: number) {
    return CHIPS[id] ?? null;
  }

  getChipCount(id: number, code: string) {
    return this.getChipCountRaw(id, CHIPS[id]!.codes!.indexOf(code));
  }

  getChipCountRaw(id: number, variant: number) {
    return this.editor.dv.getUint8(0x26e4 + ((id * 0xc) | variant));
  }

  setChipCount(id: number, code: string, n: number) {
    this.setChipCountRaw(id, CHIPS[id]!.codes!.indexOf(code), n);
  }

  setChipCountRaw(id: number, variant: number, n: number) {
    this.editor.dv.setUint8(0x26e4 + ((id * 0xc) | variant), n);
  }

  getChipRaw(folderIdx: number, chipIdx: number) {
    const chipConstant = this.editor.dv.getUint16(
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
    this.editor.dv.setUint16(
      0x262c + folderIdx * (30 * 2) + chipIdx * 2,
      id | (variant << 9),
      true
    );
  }

  setChip(folderIdx: number, chipIdx: number, id: number, code: string) {
    this.setChipRaw(folderIdx, chipIdx, id, CHIP_CODES.indexOf(code));
  }
}

export class Editor {
  dv: DataView;
  private romViewer: ROMViewer;

  getROMInfo() {
    return this.romViewer.getROMInfo();
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
    buffer = buffer.slice(0, SRAM_SIZE);
    const dv = new DataView(buffer);
    maskSave(dv);
    return buffer;
  }

  static rawToSramDump(buffer: ArrayBuffer) {
    const arr = new Uint8Array(0x10000);
    arr.set(new Uint8Array(buffer));
    maskSave(new DataView(arr.buffer));
    return arr.buffer;
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

    const startOffset = Editor.getStartOffset(buffer);
    if (startOffset == null) {
      throw "could not locate start offset";
    }
    const dv = new DataView(buffer, startOffset);

    const decoder = new TextDecoder("ascii");
    const gn = decoder.decode(
      new Uint8Array(buffer, dv.byteOffset + GAME_NAME_OFFSET, 20)
    );
    if (gn != "ROCKMANEXE4 20031022") {
      throw "unknown game name: " + gn;
    }

    const checksum = getChecksum(dv);
    const rawChecksum = computeChecksumRaw(dv);
    const firstVal = new Uint8Array(buffer, 0, 1)[0];

    const romNames = [];

    if (checksum == rawChecksum + CHECKSUM_START.bluemoon) {
      romNames.push("ROCK_EXE4_BMB4BJ");
    }

    if (checksum == rawChecksum + CHECKSUM_START.redsun) {
      romNames.push("ROCK_EXE4_RSB4WJ");
    }

    if (checksum == rawChecksum + CHECKSUM_START.bluemoon + firstVal) {
      romNames.push("MEGAMANBN4BMB4BE");
    }

    if (checksum == rawChecksum + CHECKSUM_START.redsun + firstVal) {
      romNames.push("MEGAMANBN4RSB4WE");
    }

    if (romNames.length == 0) {
      throw "unknown game, no checksum formats match";
    }

    return romNames;
  }

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, lang: string) {
    const startOffset = Editor.getStartOffset(buffer);
    if (startOffset == null) {
      throw "could not locate start offset";
    }

    this.dv = new DataView(buffer, startOffset);
    this.romViewer = new ROMViewer(romBuffer, lang);
  }

  getGameInfo() {
    return GAME_INFOS[this.getROMInfo().name];
  }

  getChecksum() {
    return getChecksum(this.dv);
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  computeChecksum() {
    return computeChecksum(this.dv, this.getGameInfo());
  }

  rebuild() {
    this.rebuildChecksum();
  }

  getRawBufferForSave() {
    if (this.getChecksum() != this.computeChecksum()) {
      throw "checksum does not match";
    }
    return this.dv.buffer;
  }

  getFolderEditor() {
    return new FolderEditor(this);
  }

  getNavicustEditor() {
    return null;
  }

  getModcardsEditor() {
    return null;
  }
}

class ROMViewer extends ROMViewerBase {}
