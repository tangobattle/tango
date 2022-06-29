const SRAM_SIZE = 0x3a78;
const GAME_NAME_OFFSET = 0x1198;
const CHECKSUM_OFFSET = 0x1148;

export class Editor {
  dv: DataView;
  private romName: string;

  constructor(buffer: ArrayBuffer, romName: string, verifyChecksum = true) {
    this.dv = new DataView(buffer);
    this.romName = romName;

    if (verifyChecksum && this.getChecksum() != this.computeChecksum()) {
      throw "checksum does not match";
    }
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

  getROMName() {
    return this.romName;
  }

  getChecksum() {
    return this.dv.getUint32(CHECKSUM_OFFSET, true);
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  computeChecksum() {
    let checksum = 0x16;
    const arr = new Uint8Array(this.dv.buffer, 0, this.dv.buffer.byteLength);
    for (let i = 0; i < this.dv.buffer.byteLength; ++i) {
      if (i == CHECKSUM_OFFSET + this.dv.byteOffset) {
        // Don't include the checksum itself in the checksum.
        i += 3;
        continue;
      }
      checksum += arr[i];
    }
    return checksum;
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

    return ["ROCKMAN_EXE2AE2J", "MEGAMAN_EXE2AE2E"];
  }

  rebuild() {
    // TODO
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
