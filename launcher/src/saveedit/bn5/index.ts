import type { Chip } from "..";
import array2d from "../../array2d";
import { ROMInfo } from "../../rom";
import { getChipIcon, getChipText, getPalette, ROMViewerBase } from "../rom";
import NCPS from "./data/ncps.json";

export interface GameInfo {
  region: "US" | "JP";
  version: "protoman" | "colonel";
}

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

const SRAM_START_OFFSET = 0x0100;
const SRAM_SIZE = 0x7c14;
const MASK_OFFSET = 0x1a34;
const GAME_NAME_OFFSET = 0x29e0;
const CHECKSUM_OFFSET = 0x29dc;

function maskSave(dv: DataView) {
  const mask = dv.getUint32(MASK_OFFSET, true);
  const unmasked = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
  for (let i = 0; i < unmasked.length; ++i) {
    unmasked[i] = (unmasked[i] ^ mask) & 0xff;
  }
  // Write the mask back.
  dv.setUint32(MASK_OFFSET, mask, true);
}

function getChecksum(dv: DataView) {
  return dv.getUint32(CHECKSUM_OFFSET, true);
}

function computeChecksum(dv: DataView, version: string) {
  let checksum = CHECKSUM_START[version];
  const arr = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
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

const CHECKSUM_START: { [key: string]: number } = {
  protoman: 0x72,
  colonel: 0x18,
};

const GAME_INFOS: { [key: string]: GameInfo } = {
  // Japan
  ROCKEXE5_TOBBRBJ: {
    region: "JP",
    version: "protoman",
  },
  ROCKEXE5_TOCBRKJ: {
    region: "JP",
    version: "colonel",
  },

  // US
  MEGAMAN5_TP_BRBE: {
    region: "US",
    version: "protoman",
  },
  MEGAMAN5_TC_BRKE: {
    region: "US",
    version: "colonel",
  },
};

const ROM_NAMES_BY_SAVE_GAME_NAME: { [key: string]: string } = {
  "REXE5TOB 20041006 US": "MEGAMAN5_TP_BRBE",
  "REXE5TOK 20041006 US": "MEGAMAN5_TC_BRKE",
  "REXE5TOB 20041104 JP": "ROCKEXE5_TOBBRBJ",
  "REXE5TOK 20041104 JP": "ROCKEXE5_TOCBRKJ",
};

class NavicustEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getNavicustProgramInfo(id: number) {
    return NCPS[id] ?? null;
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
    const offset = 0x4d6c + i * 8;
    const blockConstant = this.editor.dv.getUint8(offset);
    const id = blockConstant >> 2;
    if (id == 0) {
      return null;
    }

    return {
      id,
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
    const offset = 0x4d6c + i * 8;
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
    return this.editor.dv.getUint8(0x52d5);
  }

  setEquippedFolder(i: number) {
    return this.editor.dv.setUint8(0x52d5, i);
  }

  isRegularChipInPlace() {
    return true;
  }

  getRegularChipIndex(folderIdx: number) {
    const i = this.editor.dv.getUint8(0x52d6 + folderIdx);
    return i != 0xff ? i : null;
  }

  setRegularChipIndex(folderIdx: number, i: number | null) {
    this.editor.dv.setUint8(0x52d6 + folderIdx, i == null ? 0xff : i);
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

  getChipCountRaw(id: number, variant: number) {
    return this.editor.dv.getUint8(0x2eac + ((id * 0xc) | variant));
  }

  setChipCountRaw(id: number, variant: number, n: number) {
    this.editor.dv.setUint8(0x2eac + ((id * 0xc) | variant), n);
  }

  getChipRaw(folderIdx: number, chipIdx: number) {
    const chipConstant = this.editor.dv.getUint16(
      0x2df4 + folderIdx * (30 * 2) + chipIdx * 2,
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
      0x2df4 + folderIdx * (30 * 2) + chipIdx * 2,
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
  romViewer: ROMViewer;
  navicustDirty: boolean;

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, lang: string) {
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer, lang);
    this.navicustDirty = false;
  }

  static sramDumpToRaw(buffer: ArrayBuffer) {
    buffer = buffer.slice(SRAM_START_OFFSET, SRAM_START_OFFSET + SRAM_SIZE);
    maskSave(new DataView(buffer));
    return buffer;
  }

  static rawToSRAMDump(buffer: ArrayBuffer) {
    const arr = new Uint8Array(0x10000);
    arr.set(new Uint8Array(buffer), SRAM_START_OFFSET);
    maskSave(new DataView(arr.buffer, SRAM_START_OFFSET, SRAM_SIZE));
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
    if (
      !Object.prototype.hasOwnProperty.call(ROM_NAMES_BY_SAVE_GAME_NAME, gn)
    ) {
      throw "unknown game name: " + gn;
    }

    if (
      getChecksum(dv) !=
      computeChecksum(dv, GAME_INFOS[ROM_NAMES_BY_SAVE_GAME_NAME[gn]].version)
    ) {
      throw "checksum mismatch";
    }

    return [ROM_NAMES_BY_SAVE_GAME_NAME[gn]];
  }

  computeChecksum() {
    return computeChecksum(this.dv, this.getGameInfo().version);
  }

  getCurrentNavi() {
    return this.dv.getUint8(0x2941);
  }

  setCurrentNavi(i: number) {
    this.dv.setUint8(0x2941, i);
  }

  rebuildNavicustTiles() {
    const navicustEditor = this.getNavicustEditor();

    const arr = new Uint8Array(this.dv.buffer, this.dv.byteOffset + 0x4d48, 25);

    for (let i = 0; i < arr.length; ++i) {
      arr[i] = 0;
    }

    for (let idx = 0; idx < 30; ++idx) {
      const placement = navicustEditor.getNavicustBlock(idx);
      if (placement == null) {
        continue;
      }

      let squares = array2d.from(NCPS[placement.id]!.squares, 5, 5);
      for (let i = 0; i < placement.rot; ++i) {
        squares = array2d.rot90(squares);
      }

      for (let i = 0; i < squares.nrows; ++i) {
        for (let j = 0; j < squares.nrows; ++j) {
          const i2 = i + placement.row - 2;
          const j2 = j + placement.col - 2;
          if (i2 >= 5 || j2 >= 5) {
            continue;
          }
          const v = squares[i * squares.ncols + j];
          if (v == 0) {
            continue;
          }
          if (placement.compressed && v != 1) {
            continue;
          }
          arr[i2 * 5 + j2] = idx + 1;
        }
      }
    }
    this.navicustDirty = false;
  }

  rebuild() {
    this.rebuildChecksum();
  }

  getChecksum() {
    this.rebuildNavicustTiles();
    return getChecksum(this.dv);
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
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

  getModcardsEditor() {
    return null;
  }
}

interface ROMOffsets {
  chipData: number;
  chipIconPalette: number;
  chipNamesPointers: number;
}

class ROMViewer extends ROMViewerBase {
  private offsets: ROMOffsets;
  private palette: Uint32Array;

  constructor(buffer: ArrayBuffer, lang: string) {
    super(buffer, lang);
    this.offsets = getOffsets(this.getROMInfo());
    this.palette = getPalette(this.dv, this.offsets.chipIconPalette);
  }

  getChipInfo(id: number): Chip {
    const dataOffset = this.offsets.chipData + id * 0x2c;

    const codes = [];
    for (let i = 0; i < 4; ++i) {
      const code = this.dv.getUint8(dataOffset + 0x00 + i);
      if (code == 0xff) {
        continue;
      }
      codes.push(CHIP_CODES[code]);
    }
    const flags = this.dv.getUint8(dataOffset + 0x09);

    return {
      name: {
        en: getChipString(
          this.dv,
          this.lang,
          this.offsets.chipNamesPointers,
          id
        ),
      },
      codes: codes.join(""),
      icon: getChipIcon(
        this.dv,
        this.palette,
        this.dv.getUint32(dataOffset + 0x20, true) & ~0x08000000
      ),
      element: this.dv.getUint8(dataOffset + 0x06).toString(),
      class: ["standard", "mega", "giga"][this.dv.getUint8(dataOffset + 0x07)],
      mb: this.dv.getUint8(dataOffset + 0x08),
      damage: (flags & 0x2) != 0 ? this.dv.getUint8(dataOffset + 0x1a) : 0,
    };
  }
}

function getOffsets(romInfo: ROMInfo): ROMOffsets {
  switch (
    `${romInfo.name}_${romInfo.revision
      .toString(16)
      .toUpperCase()
      .padStart(2, "0")}`
  ) {
    case "MEGAMAN5_TP_BRBE_00":
      return {
        chipData: 0x0001e214,
        chipIconPalette: 0x0074aab8,
        chipNamesPointers: 0x00023b1c,
      };
    case "MEGAMAN5_TC_BRKE_00":
      return {
        chipData: 0x0001e210,
        chipIconPalette: 0x0074bdbc,
        chipNamesPointers: 0x00023b18,
      };
  }
  throw `unknown rom: ${romInfo.name}`;
}

const CHARSETS = {
  en: " 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ*abcdefghijklmnopqrstuvwxyzウアイオエケコカクキセサソシステトツタチネノヌナニヒヘホハフミマメムモヤヨユロルリレラン熱斗ワヲギガゲゴグゾジゼズザデドヅダヂベビボバブピパペプポゥァィォェュヴッョャ-×=:%?+█�ー!�&,。.・;'\"~/()「」αβ�■_�周えおうあいけくきこかせそすさしつとてたちねのなぬにへふほはひめむみもまゆよやるらりろれ究んをわ",
  ja: ' 0123456789ウアイオエケコカクキセサソシステトツタチネノヌナニヒヘホハフミマメムモヤヨユロルリレラン熱斗ワヲギガゲゴグゾジゼズザデドヅダヂベビボバブピパペプポゥァィォェュヴッョャABCDEFGHIJKLMNOPQRSTUVWXYZ*-×=:%?+■�ー!�&、゜.・;’"~/()「」����_�周えおうあいけくきこかせそすさしつとてたちねのなぬにへふほはひめむみもまゆよやるらりろれ究んをわ研げぐごがぎぜずじぞざでどづだぢべばびぼぶぽぷぴぺぱ',
};

function getChipString(
  dv: DataView,
  lang: string,
  scriptPointerOffset: number,
  id: number
): string {
  const charset = CHARSETS[lang as keyof typeof CHARSETS];
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
