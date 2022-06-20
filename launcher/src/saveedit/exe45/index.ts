import CHIPS from "./data/chips.json";

export { CHIPS };

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

const SRAM_START_OFFSET = 0x0100;
const SRAM_SIZE = 0xc7a8;
const MASK_OFFSET = 0x3b84;
const GAME_NAME_OFFSET = 0x4aa8;
const CHECKSUM_OFFSET = 0x4a88;

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

  getChipData() {
    return CHIPS;
  }

  getChipCount(id: number, code: string) {
    return this.getChipCountRaw(id, CHIPS[id]!.codes!.indexOf(code));
  }

  getChipCountRaw(id: number, variant: number) {
    return this.editor.dv.getUint8(0x52c8 + ((id * 0xc) | variant));
  }

  setChipCount(id: number, code: string, n: number) {
    this.setChipCountRaw(id, CHIPS[id]!.codes!.indexOf(code), n);
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
}

export class Editor {
  dv: DataView;
  private romName: string;

  getROMName() {
    return this.romName;
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
    if (gn != "ROCKMANEXE4RO 040607") {
      throw "unknown game name: " + gn;
    }

    return ["ROCKEXE4.5ROBR4J"];
  }

  constructor(buffer: ArrayBuffer, romName: string, verifyChecksum = true) {
    this.dv = new DataView(buffer);
    this.romName = romName;

    if (verifyChecksum && this.getChecksum() != this.computeChecksum()) {
      throw "checksum does not match";
    }
  }

  getChecksum() {
    return this.dv.getUint32(CHECKSUM_OFFSET, true);
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  computeChecksum() {
    let checksum = 0x38;
    const arr = new Uint8Array(
      this.dv.buffer,
      this.dv.byteOffset,
      this.dv.buffer.byteLength
    );
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
