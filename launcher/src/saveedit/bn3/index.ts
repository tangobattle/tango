import { ROMViewerBase } from "../rom";

export interface GameInfo {
  region: "US" | "JP";
  version: "white" | "blue";
}

const SRAM_SIZE = 0x57b0;
const GAME_NAME_OFFSET = 0x1e00;
const CHECKSUM_OFFSET = 0x1dd8;

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

export class Editor {
  dv: DataView;
  private romViewer: ROMViewer;

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, _saveeditInfo: any) {
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer);
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
    return null;
  }

  getNavicustEditor() {
    return null;
  }

  getModcardsEditor() {
    return null;
  }
}

class ROMViewer extends ROMViewerBase {}
