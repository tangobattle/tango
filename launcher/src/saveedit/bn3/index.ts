export interface GameInfo {
  region: "US" | "JP";
  version: "white" | "blue";
}

const GAME_NAME_OFFSET = 0x1e00;

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

export class Editor {
  dv: DataView;
  private romName: string;

  constructor(buffer: ArrayBuffer, romName: string, _verifyChecksum = true) {
    this.dv = new DataView(buffer);
    this.romName = romName;
  }

  static sramDumpToRaw(buffer: ArrayBuffer) {
    // TODO
    return buffer;
  }

  static rawToSRAMDump(buffer: ArrayBuffer) {
    // TODO
    return buffer;
  }

  getROMName() {
    return this.romName;
  }

  getGameFamily() {
    return "bn3";
  }

  static sniffROMNames(buffer: ArrayBuffer) {
    const decoder = new TextDecoder("ascii");
    const gn = decoder.decode(new Uint8Array(buffer, 0 + GAME_NAME_OFFSET, 20));
    if (gn != "ROCKMANEXE3 20021002" && gn != "BBN3 v0.5.0 20021002") {
      throw "unknown game name: " + gn;
    }
    return [
      "ROCKMAN_EXE3A6BJ",
      "ROCK_EXE3_BKA3XJ",
      "MEGA_EXE3_BLA3XE",
      "MEGA_EXE3_WHA6BE",
    ];
  }

  rebuild() {
    // TODO
  }

  getGameInfo() {
    return GAME_INFOS[this.romName];
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
