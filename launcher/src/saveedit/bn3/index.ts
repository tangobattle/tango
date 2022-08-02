import { Chip, NavicustProgram } from "../";
import array2d from "../../array2d";
import { EditorBase } from "../base";
import { getChipText, getPalette, getTextSimple, getTiles, ROMViewerBase } from "../rom";

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

export interface GameInfo {
  region: "US" | "JP";
  version: "white" | "blue";
}

const SRAM_SIZE = 0x57b0;
const GAME_NAME_OFFSET = 0x1e00;
const CHECKSUM_OFFSET = 0x1dd8;

const PARSE_TEXT_OPTIONS = {
  controlCodeHandlers: {
    0xe7: (_dv: DataView, _offset: number) => {
      return null;
    },
    0xe8: (_dv: DataView, offset: number) => {
      return { offset, control: { c: "newline" } };
    },
  },
  extendCharsetControlCode: 0xe5,
};

const GAME_INFOS: { [key: string]: GameInfo } = {
  // Japan
  ROCKMAN_EXE3A6BJ: {
    region: "JP",
    version: "white",
  },
  ROCK_EXE3_BKA3XJ: {
    region: "JP",
    version: "blue",
  },

  // US
  MEGA_EXE3_WHA6BE: {
    region: "US",
    version: "white",
  },
  MEGA_EXE3_BLA3XE: {
    region: "US",
    version: "blue",
  },
};

const CHECKSUM_START: { [key: string]: number } = {
  white: 0x16,
  blue: 0x22,
};

function getChecksum(dv: DataView) {
  return dv.getUint32(CHECKSUM_OFFSET, true);
}

function computeChecksum(dv: DataView, version: string) {
  return computeChecksumRaw(dv) + CHECKSUM_START[version];
}

function computeChecksumRaw(dv: DataView) {
  let checksum = 0;
  const arr = new Uint8Array(dv.buffer, 0, dv.buffer.byteLength);
  for (let i = 0; i < dv.buffer.byteLength; ++i) {
    if (i == CHECKSUM_OFFSET + dv.byteOffset) {
      // Don't include the checksum itself in the checksum.
      i += 3;
      continue;
    }
    checksum += arr[i];
  }
  return checksum;
}

class NavicustEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getNavicustProgramInfo(id: number, variant: number) {
    return this.editor.romViewer.getNavicustProgramInfo(id, variant);
  }

  getCommandLine() {
    return 2;
  }

  hasOutOfBounds() {
    return false;
  }

  getWidth() {
    return 5;
  }

  getHeight() {
    return 5;
  }

  getNavicustBlock(i: number) {
    const offset = 0x1300 + i * 8;
    const blockConstant = this.editor.dv.getUint8(offset);
    if (blockConstant == 0) {
      return null;
    }

    const id = blockConstant >> 2;

    return {
      id,
      variant: blockConstant & 0x3,
      col: this.editor.dv.getUint8(offset + 2),
      row: this.editor.dv.getUint8(offset + 3),
      rot: this.editor.dv.getUint8(offset + 4),
      compressed: true,
    };
  }

  setNavicustBlock(
    i: number,
    id: number,
    variant: number,
    col: number,
    row: number,
    rot: number,
    compressed: boolean
  ) {
    const offset = 0x1300 + i * 8;
    this.editor.dv.setUint8(offset, (id << 2) | variant);
    this.editor.dv.setUint8(offset + 2, col);
    this.editor.dv.setUint8(offset + 3, row);
    this.editor.dv.setUint8(offset + 4, rot);
    this.editor.dv.setUint8(offset + 5, compressed ? 1 : 0);
    this.editor.navicustDirty = true;
  }
}

class FolderEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getFolderCount() {
    return 3;
  }

  getEquippedFolder() {
    return this.editor.dv.getUint8(0x1882);
  }

  isRegularChipInPlace() {
    return true;
  }

  getRegularChipIndex(folderIdx: number) {
    const i = this.editor.dv.getUint8(0x189d + folderIdx);
    return i != 0xff ? i : null;
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
    return this.editor.romViewer.getChipInfo(id);
  }

  getChipRaw(folderIdx: number, chipIdx: number) {
    return {
      id: this.editor.dv.getUint16(
        0x1410 + folderIdx * (30 * 4) + chipIdx * 4,
        true
      ),
      variant: this.editor.dv.getUint16(
        0x1410 + folderIdx * (30 * 4) + chipIdx * 4 + 2,
        true
      ),
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

  getElementIcons() {
    return this.editor.romViewer.getElementIcons();
  }
}

export class Editor extends EditorBase {
  dv: DataView;
  romViewer: ROMViewer;
  navicustDirty: boolean;

  constructor(
    buffer: ArrayBuffer,
    romBuffer: ArrayBuffer,
    saveeditInfo: SaveeditInfo
  ) {
    super();
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer, saveeditInfo);
    this.navicustDirty = false;
  }

  static sramDumpToRaw(buffer: ArrayBuffer) {
    buffer = buffer.slice(0, SRAM_SIZE);
    return buffer;
  }

  static rawToSRAMDump(buffer: ArrayBuffer) {
    const arr = new Uint8Array(0x10000);
    arr.set(new Uint8Array(buffer));
    return arr.buffer;
  }

  getROMInfo() {
    return this.romViewer.getROMInfo();
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
    if (gn != "ROCKMANEXE3 20021002" && gn != "BBN3 v0.5.0 20021002") {
      throw "unknown game name: " + gn;
    }

    const checksum = getChecksum(dv);
    const computedChecksum = computeChecksumRaw(dv);

    const romNames = [];

    if (checksum == computedChecksum + CHECKSUM_START.white) {
      romNames.push("ROCKMAN_EXE3A6BJ", "MEGA_EXE3_WHA6BE");
    }

    if (checksum == computedChecksum + CHECKSUM_START.blue) {
      romNames.push("ROCK_EXE3_BKA3XJ", "MEGA_EXE3_BLA3XE");
    }

    if (romNames.length == 0) {
      throw "unknown game, no checksum formats match";
    }

    return romNames;
  }

  getChecksum() {
    return getChecksum(this.dv);
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  computeChecksum() {
    return computeChecksum(this.dv, this.getGameInfo().version);
  }

  rebuild() {
    // TODO
  }

  getGameInfo() {
    return GAME_INFOS[this.getROMInfo().name];
  }

  getFolderEditor() {
    return new FolderEditor(this);
  }

  getNavicustEditor() {
    return new NavicustEditor(this);
  }
}

interface SaveeditInfo {
  charset: string;
  offsets: {
    chipData: number;
    chipNamesPointers: number;
    chipIconPalettePointer: number;
    elementIconPalettePointer: number;
    elementIconsPointer: number;
    ncpData: number;
    ncpNamesPointer: number;
  };
}

class ROMViewer extends ROMViewerBase {
  private saveeditInfo: SaveeditInfo;
  private chipIconPalette: Uint32Array;
  private elementIconPalette: Uint32Array;

  constructor(buffer: ArrayBuffer, saveeditInfo: SaveeditInfo) {
    super(buffer);
    this.saveeditInfo = saveeditInfo;
    this.chipIconPalette = getPalette(
      this.dv,
      this.dv.getUint32(
        this.saveeditInfo.offsets.chipIconPalettePointer,
        true
      ) & ~0x08000000
    );
    this.elementIconPalette = getPalette(
      this.dv,
      this.dv.getUint32(saveeditInfo.offsets.elementIconPalettePointer, true) &
        ~0x08000000
    );
  }

  getElementIcons(): ImageData[] {
    const icons: ImageData[] = [];
    const start =
      this.dv.getUint32(this.saveeditInfo.offsets.elementIconsPointer, true) &
      ~0x08000000;
    for (let i = 0; i < 5; ++i) {
      icons.push(
        getTiles(
          this.dv,
          this.elementIconPalette,
          start + 0x1e0 + i * 0x80,
          2,
          2
        )
      );
    }
    return icons;
  }

  getChipInfo(id: number): Chip {
    const dataOffset = this.saveeditInfo.offsets.chipData + id * 0x20;

    const codes = [];
    for (let i = 0; i < 6; ++i) {
      const code = this.dv.getUint8(dataOffset + 0x00 + i);
      if (code == 0xff) {
        continue;
      }
      codes.push(CHIP_CODES[code]);
    }

    const damage = this.dv.getUint16(dataOffset + 0x0c, true);
    const flags = this.dv.getUint8(dataOffset + 0x13);

    return {
      name: getChipText(
        this.dv,
        this.saveeditInfo.offsets.chipNamesPointers,
        id,
        this.saveeditInfo.charset,
        PARSE_TEXT_OPTIONS
      ),
      codes: codes.join(""),
      icon: getTiles(
        this.dv,
        this.chipIconPalette,
        this.dv.getUint32(dataOffset + 0x14, true) & ~0x08000000,
        2,
        2
      ),
      element: this.dv.getUint8(dataOffset + 0x06),
      class:
        (flags & 0x2) != 0 ? "giga" : (flags & 0x1) != 0 ? "mega" : "standard",
      mb: this.dv.getUint8(dataOffset + 0x0a),
      damage: damage < 1000 ? damage : 0,
    };
  }

  getNavicustProgramInfo(id: number, variant: number): NavicustProgram {
    const dataOffset = this.saveeditInfo.offsets.ncpData + id * 0x40;

    const subdataOffset = dataOffset + variant * 0x10;

    return {
      name: getTextSimple(
        this.dv,
        this.dv.getUint32(this.saveeditInfo.offsets.ncpNamesPointer, true) &
          ~0x08000000,
        id,
        this.saveeditInfo.charset,
        PARSE_TEXT_OPTIONS
      ),
      color: [
        null,
        "white",
        "pink",
        "yellow",
        "red",
        "blue",
        "green",
        "orange",
        "purple",
        "gray",
      ][this.dv.getUint8(subdataOffset + 0x3)] as NavicustProgram["color"],
      isSolid: this.dv.getUint8(subdataOffset + 0x1) == 0,
      uncompressed: array2d.from(
        [
          ...new Uint8Array(
            this.dv.buffer,
            this.dv.getUint32(subdataOffset + 0x8, true) & ~0x08000000,
            5 * 5
          ),
        ].map((v) => !!v),
        5,
        5
      ),
      compressed: array2d.from(
        [
          ...new Uint8Array(
            this.dv.buffer,
            this.dv.getUint32(subdataOffset + 0xc, true) & ~0x08000000,
            5 * 5
          ),
        ].map((v) => !!v),
        5,
        5
      ),
    };
  }
}
