import type { Chip } from "..";
import { getChipIcon, getChipText, getPalette, ROMViewerBase } from "../rom";

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

const SRAM_START_OFFSET = 0x00;
const SRAM_SIZE = 0xc7a8;
const MASK_OFFSET = 0x3c84;
const GAME_NAME_OFFSET = 0x4ba8;
const CHECKSUM_OFFSET = 0x4b88;

function getChecksum(dv: DataView) {
  return dv.getUint32(CHECKSUM_OFFSET, true);
}

function computeChecksum(dv: DataView) {
  let checksum = 0x38;
  const arr = new Uint8Array(dv.buffer, dv.byteOffset, dv.buffer.byteLength);
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

function maskSave(dv: DataView) {
  const mask = dv.getUint32(MASK_OFFSET, true);
  const unmasked = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
  for (let i = 0; i < unmasked.length; ++i) {
    unmasked[i] = (unmasked[i] ^ mask) & 0xff;
  }
  // Write the mask back.
  dv.setUint32(MASK_OFFSET, mask, true);
}

class FolderEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getFolderCount() {
    return 1; // TODO
  }

  getEquippedFolder() {
    return 0;
  }

  setEquippedFolder(_i: number) {
    // Not supported.
    return;
  }

  isRegularChipInPlace() {
    return false;
  }

  getRegularChipIndex(_folderIdx: number) {
    // Not supported.
    return null;
  }

  setRegularChipIndex(_folderIdx: number, _i: number | null) {
    // Not supported.
    return null;
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
    return this.editor.dv.getUint8(0x52c8 + ((id * 0xc) | variant));
  }

  setChipCount(id: number, code: string, n: number) {
    const chip = this.getChipInfo(id);
    this.setChipCountRaw(id, chip.codes!.indexOf(code), n);
  }

  setChipCountRaw(id: number, variant: number, n: number) {
    this.editor.dv.setUint8(0x52c8 + ((id * 0xc) | variant), n);
  }

  getChipRaw(folderIdx: number, chipIdx: number) {
    const naviIdx = this.editor.dv.getUint8(0x4ad1);
    const chipConstant = this.editor.dv.getUint16(
      0x7500 + naviIdx * (30 * 2) + chipIdx * 2,
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
    const naviIdx = this.editor.dv.getUint8(0x4ad1);
    this.editor.dv.setUint16(
      0x7500 + naviIdx * (30 * 2) + chipIdx * 2,
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

  getROMInfo() {
    return this.romViewer.getROMInfo();
  }

  static sramDumpToRaw(buffer: ArrayBuffer) {
    buffer = buffer.slice(SRAM_START_OFFSET, SRAM_START_OFFSET + SRAM_SIZE);
    maskSave(new DataView(buffer));
    return buffer;
  }

  static rawToSramDump(buffer: ArrayBuffer) {
    const arr = new Uint8Array(0x10000);
    arr.set(new Uint8Array(buffer), SRAM_START_OFFSET);
    maskSave(new DataView(arr.buffer, SRAM_START_OFFSET, SRAM_SIZE));
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

    const dv = new DataView(buffer);

    const decoder = new TextDecoder("ascii");
    const gn = decoder.decode(
      new Uint8Array(buffer, dv.byteOffset + GAME_NAME_OFFSET, 20)
    );
    if (gn != "ROCKMANEXE4RO 040607" && gn != "ROCKMANEXE4RO 041217") {
      throw "unknown game name: " + gn;
    }

    if (computeChecksum(dv) != getChecksum(dv)) {
      throw "checksum mismatch";
    }

    return ["ROCKEXE4.5ROBR4J"];
  }

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, saveeditInfo: any) {
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer, this.dv, saveeditInfo);
  }

  getChecksum() {
    return getChecksum(this.dv);
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  computeChecksum() {
    return computeChecksum(this.dv);
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

interface SaveeditInfo {
  charset: string;
  offsets: {
    chipData: number;
    chipIconPalettePointer: number;
    chipNamesPointers: number;
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
