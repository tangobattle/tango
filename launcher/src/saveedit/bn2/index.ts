import { EditorBase } from "../base";
import { getChipText, getPalette, getTiles, ROMViewerBase } from "../rom";

import type { Chip } from "../";

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

const SRAM_SIZE = 0x3a78;
const GAME_NAME_OFFSET = 0x1198;
const CHECKSUM_OFFSET = 0x114c;

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

function getChecksum(dv: DataView) {
  return dv.getUint32(CHECKSUM_OFFSET, true);
}

function computeChecksum(dv: DataView) {
  let checksum = 0x16;
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

class FolderEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getFolderCount() {
    return 3;
  }

  getEquippedFolder() {
    return this.editor.dv.getUint8(0x0dc2);
  }

  isRegularChipInPlace() {
    return true;
  }

  getRegularChipIndex(folderIdx: number) {
    const i = this.editor.dv.getUint8(0x0ddd + folderIdx);
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
        0x0ab0 + folderIdx * (30 * 4) + chipIdx * 4,
        true
      ),
      variant: this.editor.dv.getUint16(
        0x0ab0 + folderIdx * (30 * 4) + chipIdx * 4 + 2,
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

  constructor(
    buffer: ArrayBuffer,
    romBuffer: ArrayBuffer,
    saveeditInfo: SaveeditInfo
  ) {
    super();
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer, saveeditInfo);
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

  getChecksum() {
    return getChecksum(this.dv);
  }

  getROMInfo() {
    return this.romViewer.getROMInfo();
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  computeChecksum() {
    return computeChecksum(this.dv);
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
    if (gn != "ROCKMANEXE2 20011016") {
      throw "unknown game name: " + gn;
    }

    if (computeChecksum(dv) != getChecksum(dv)) {
      throw "checksum mismatch";
    }

    return ["ROCKMAN_EXE2AE2J", "MEGAMAN_EXE2AE2E"];
  }

  rebuild() {
    // TODO
  }

  getFolderEditor() {
    return new FolderEditor(this);
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
        getTiles(this.dv, this.elementIconPalette, start + i * 0x80, 2, 2)
      );
    }
    return icons;
  }

  getChipInfo(id: number): Chip {
    const dataOffset = this.saveeditInfo.offsets.chipData + id * 0x20;

    const codes = [];
    for (let i = 0; i < 5; ++i) {
      const code = this.dv.getUint8(dataOffset + 0x00 + i);
      if (code == 0xff) {
        continue;
      }
      codes.push(CHIP_CODES[code]);
    }

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
      class: "standard",
      mb: this.dv.getUint8(dataOffset + 0x0a),
      damage: this.dv.getUint16(dataOffset + 0x0c, true),
    };
  }
}
