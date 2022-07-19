const SRAM_SIZE = 0x3a78;
const GAME_NAME_OFFSET = 0x1198;
const CHECKSUM_OFFSET = 0x114c;

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

export class Editor {
  dv: DataView;
  private romName: string;

  constructor(
    buffer: ArrayBuffer,
    romBuffer: ArrayBuffer,
    romName: string,
    _lang: string | null
  ) {
    this.dv = new DataView(buffer);
    this.romName = romName;
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

  getChecksum(dv: DataView) {
    return getChecksum(dv);
  }

  getROMName() {
    return this.romName;
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
    return null;
  }

  getNavicustEditor() {
    return null;
  }

  getModcardsEditor() {
    return null;
  }
}
