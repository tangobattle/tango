import type { Chip, NavicustProgram } from "../";
import array2d from "../../array2d";
import { getChipIcon, getChipText, getPalette, getText, ROMViewerBase } from "../rom";

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
    const offset = 0x4564 + i * 8;
    const blockConstant = this.editor.dv.getUint8(offset);
    if (blockConstant == 0) {
      return null;
    }

    return {
      id: blockConstant >> 2,
      variant: blockConstant & 0x3,
      col: this.editor.dv.getUint8(offset + 2),
      row: this.editor.dv.getUint8(offset + 3),
      rot: this.editor.dv.getUint8(offset + 4),
      compressed: !!this.editor.dv.getUint8(offset + 5),
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
    const offset = 0x4564 + i * 8;
    this.editor.dv.setUint8(offset, (id << 2) | variant);
    this.editor.dv.setUint8(offset + 3, col);
    this.editor.dv.setUint8(offset + 4, row);
    this.editor.dv.setUint8(offset + 5, rot);
    this.editor.dv.setUint8(offset + 6, compressed ? 1 : 0);
    this.editor.navicustDirty = true;
  }
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
    return this.editor.romViewer.getChipInfo(id);
  }

  getChipCount(id: number, code: string) {
    const chip = this.getChipInfo(id);
    return this.getChipCountRaw(id, chip.codes!.indexOf(code));
  }

  getChipCountRaw(id: number, variant: number) {
    return this.editor.dv.getUint8(0x26e4 + ((id * 0xc) | variant));
  }

  setChipCount(id: number, code: string, n: number) {
    const chip = this.getChipInfo(id);
    this.setChipCountRaw(id, chip.codes!.indexOf(code), n);
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

  getElementIcons() {
    return this.editor.romViewer.getElementIcons();
  }
}

export class Editor {
  dv: DataView;
  romViewer: ROMViewer;
  navicustDirty: boolean;

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

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, saveeditInfo: any) {
    const startOffset = Editor.getStartOffset(buffer);
    if (startOffset == null) {
      throw "could not locate start offset";
    }

    this.dv = new DataView(buffer, startOffset);
    this.romViewer = new ROMViewer(romBuffer, this.dv, saveeditInfo);
    this.navicustDirty = false;
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
    return new NavicustEditor(this);
  }

  getModcardsEditor() {
    return null;
  }
}

interface SaveeditInfo {
  charset: string;
  offsets: {
    chipData: number;
    chipIconPalettePointer: number;
    chipNamesPointers: number;
    ncpData: number;
    ncpNamesPointer: number;
    elementIconPalettePointer: number;
    elementIconsPointer: number;
  };
}

class ROMViewer extends ROMViewerBase {
  private chipIconPalette: Uint32Array;
  private elementIconPalette: Uint32Array;
  private saveDv: DataView;
  private saveeditInfo: SaveeditInfo;

  constructor(
    buffer: ArrayBuffer,
    saveDv: DataView,
    saveeditInfo: SaveeditInfo
  ) {
    super(buffer);
    this.saveDv = saveDv;
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
    for (let i = 0; i < 13; ++i) {
      icons.push(
        getChipIcon(this.dv, this.elementIconPalette, start + i * 0x80)
      );
    }
    return icons;
  }

  getChipInfo(id: number): Chip {
    const dataOffset = this.saveeditInfo.offsets.chipData + id * 0x2c;

    const codes = [];
    for (let i = 0; i < 4; ++i) {
      const code = this.dv.getUint8(dataOffset + 0x00 + i);
      if (code == 0xff) {
        continue;
      }
      codes.push(CHIP_CODES[code]);
    }
    const flags = this.dv.getUint8(dataOffset + 0x09);
    const iconPtr = this.dv.getUint32(dataOffset + 0x20, true);

    return {
      name: getChipString(
        this.dv,
        this.saveeditInfo.charset,
        this.saveeditInfo.offsets.chipNamesPointers,
        id
      ),
      codes: codes.join(""),
      icon:
        iconPtr >= 0x08000000
          ? getChipIcon(this.dv, this.chipIconPalette, iconPtr & ~0x08000000)
          : getChipIcon(
              this.saveDv,
              this.chipIconPalette,
              iconPtr & ~0x02000000
            ),
      element: this.dv.getUint8(dataOffset + 0x07),
      class: ["standard", "mega", "giga"][this.dv.getUint8(dataOffset + 0x08)],
      mb: this.dv.getUint8(dataOffset + 0x06),
      damage: (flags & 0x2) != 0 ? this.dv.getUint8(dataOffset + 0x1a) : 0,
    };
  }

  getNavicustProgramInfo(id: number, variant: number): NavicustProgram {
    const dataOffset = this.saveeditInfo.offsets.ncpData + id * 0x40;

    const subdataOffset = dataOffset + variant * 0x10;

    return {
      name: getText(
        this.dv,
        this.dv.getUint32(this.saveeditInfo.offsets.ncpNamesPointer, true) &
          ~0x08000000,
        id
      )
        .map((c) => this.saveeditInfo.charset[c])
        .join(""),
      color: [null, "white", "yellow", "pink", "red", "blue", "green"][
        this.dv.getUint8(subdataOffset + 0x3)
      ] as NavicustProgram["color"],
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

function getChipString(
  dv: DataView,
  charset: string,
  scriptPointerOffset: number,
  id: number
): string {
  return getChipText(dv, scriptPointerOffset, id)
    .map((c) => charset[c])
    .join("")
    .replace(/[\u3000-\ue004]/g, (c) => {
      switch (c) {
        case "\ue001":
          return "SP";
        case "\ue002":
          return "DS";
      }
      return c;
    });
}
