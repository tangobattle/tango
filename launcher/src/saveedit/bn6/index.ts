import type { Chip } from "..";

import array2d from "../../array2d";
import { ROMInfo } from "../../rom";
import { getChipIcon, getChipText, getPalette, ROMViewerBase } from "../rom";
import MODCARDS from "./data/modcards.json";
import NCPS from "./data/ncps.json";

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

const CHARSETS = {
  en: " 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ*abcdefghijklmnopqrstuvwxyz\ue000\ue001\ue002\ue003\ue004ウアイオエケコカクキセサソシステトツタチネノヌナニヒヘホハフミマメムモヤヨユロルリレラン熱斗ワヲギガゲゴグゾジゼズザデドヅダヂベビボバブピパペプポゥァィォェュヴッョャ-×=:%?+█�ー!&,゜.・;'\"~/()「」�_�����あいけくきこかせそすさしつとてたちねのなぬにへふほはひめむみもまゆよやるらりろれ�んをわ研げぐごがぎぜずじぞざでどづだぢべばびぼぶぽぷぴぺぱぅぁぃぉぇゅょっゃ容量全木�無現実◯✗緑道不止彩起父集院一二三四五六七八陽十百千万脳上下左右手来日目月獣各人入出山口光電気綾科次名前学校省祐室世界高朗枚野悪路闇大小中自分間系花問究門城王兄化葉行街屋水見終新桜先生長今了点井子言太属風会性持時勝赤代年火改計画職体波回外地員正造値合戦川秋原町晴用金郎作数方社攻撃力同武何発少教以白早暮面組後文字本階明才者向犬々ヶ連射舟戸切土炎伊夫鉄国男天老師堀杉士悟森霧麻剛垣★[].",
  ja: ' 0123456789ウアイオエケコカクキセサソシステトツタチネノヌナニヒヘホハフミマメムモヤヨユロルリレラン熱斗ワヲギガゲゴグゾジゼズザデドヅダヂベビボバブピパペプポゥァィォェュヴッョャABCDEFGHIJKLMNOPQRSTUVWXYZ*-×=:%?+■�ー!\ue000\ue001&、゜.・;’"~/()「」\ue002\ue003\ue004_�周えおうあいけくきこかせそすさしつとてたちねのなぬにへふほはひめむみもまゆよやるらりろれ�んをわ研げぐごがぎぜずじぞざでどづだぢべばびぼぶぽぷぴぺぱぅぁぃぉぇゅょっゃabcdefghijklmnopqrstuvwxyz容量全木�無現実◯✗緑道不止彩起父集院一二三四五六七八陽十百千万脳上下左右手来日目月獣各人入出山口光電気綾科次名前学校省祐室世界高朗枚野悪路闇大小中自分間系花問究門城王兄化葉行街屋水見終新桜先生長今了点井子言太属風会性持時勝赤代年火改計画職体波回外地員正造値合戦川秋原町晴用金郎作数方社攻撃力同武何発少教以白早暮面組後文字本階明才者向犬々ヶ連射舟戸切土炎伊夫鉄国男天老師堀杉士悟森霧麻剛垣',
};

const SRAM_START_OFFSET = 0x0100;
const SRAM_SIZE = 0x6710;
const MASK_OFFSET = 0x1064;
const GAME_NAME_OFFSET = 0x1c70;
const CHECKSUM_OFFSET = 0x1c6c;

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

function maskSave(dv: DataView) {
  const mask = dv.getUint32(MASK_OFFSET, true);
  const unmasked = new Uint8Array(dv.buffer, dv.byteOffset, dv.byteLength);
  for (let i = 0; i < unmasked.length; ++i) {
    // We only actually need to use the first byte of the mask, even though it's 32 bits long.
    unmasked[i] = (unmasked[i] ^ mask) & 0xff;
  }
  // Write the mask back.
  dv.setUint32(MASK_OFFSET, mask, true);
}

const CHECKSUM_START: { [key: string]: number } = {
  falzar: 0x18,
  gregar: 0x72,
};

const ROM_NAMES_BY_SAVE_GAME_NAME: { [key: string]: string } = {
  "REXE6 F 20050924a JP": "ROCKEXE6_RXXBR6J",
  "REXE6 G 20050924a JP": "ROCKEXE6_GXXBR5J",
  "REXE6 F 20060110a US": "MEGAMAN6_FXXBR6E",
  "REXE6 G 20060110a US": "MEGAMAN6_GXXBR5E",
  "REXE6 F 20060110a PL": "MEGAMAN6_FXXBR6P",
  "REXE6 G 20060110a PL": "MEGAMAN6_GXXBR5P",
};

const GAME_INFOS: { [key: string]: GameInfo } = {
  // Japan
  ROCKEXE6_RXXBR6J: {
    region: "JP",
    version: "falzar",
  },
  ROCKEXE6_GXXBR5J: {
    region: "JP",
    version: "gregar",
  },

  // US
  MEGAMAN6_FXXBR6E: {
    region: "US",
    version: "falzar",
  },
  MEGAMAN6_GXXBR5E: {
    region: "US",
    version: "gregar",
  },

  // Poland :^)
  MEGAMAN6_FXXBR6P: {
    region: "PL",
    version: "falzar",
  },
  MEGAMAN6_GXXBR5P: {
    region: "PL",
    version: "gregar",
  },
};

export interface GameInfo {
  region: "US" | "JP" | "PL";
  version: "falzar" | "gregar";
}

class FolderEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getChipInfo(id: number) {
    return this.editor.romViewer.getChipInfo(id);
  }

  getChipCount(id: number, code: string) {
    const chip = this.getChipInfo(id);
    return this.getChipCountRaw(id, chip.codes!.indexOf(code));
  }

  getChipCountRaw(id: number, variant: number) {
    return this.editor.dv.getUint8(0x2230 + ((id * 0xc) | variant));
  }

  setChipCount(id: number, code: string, n: number) {
    const chip = this.getChipInfo(id);
    this.setChipCountRaw(id, chip.codes!.indexOf(code), n);
  }

  setChipCountRaw(id: number, variant: number, n: number) {
    this.editor.dv.setUint8(0x2230 + ((id * 0xc) | variant), n);
  }

  getFolderCount() {
    return this.editor.dv.getUint8(0x1c09);
  }

  getChipRaw(folderIdx: number, chipIdx: number) {
    const chipConstant = this.editor.dv.getUint16(
      0x2178 + folderIdx * (30 * 2) + chipIdx * 2,
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
      0x2178 + folderIdx * (30 * 2) + chipIdx * 2,
      id | (variant << 9),
      true
    );
  }

  setChip(folderIdx: number, chipIdx: number, id: number, code: string) {
    this.setChipRaw(folderIdx, chipIdx, id, CHIP_CODES.indexOf(code));
  }

  getEquippedFolder() {
    return this.editor.dv.getUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) + 0x2d
    );
  }

  setEquippedFolder(i: number) {
    return this.editor.dv.setUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) + 0x2d,
      i
    );
  }

  isRegularChipInPlace() {
    return true;
  }

  getRegularChipIndex(folderIdx: number) {
    const i = this.editor.dv.getUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) +
        0x2e +
        folderIdx
    );
    return i != 0xff ? i : null;
  }

  setRegularChipIndex(folderIdx: number, i: number) {
    this.editor.dv.setUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) +
        0x2e +
        folderIdx,
      i != null ? i : 0xff
    );
  }

  getTagChip1Index(folderIdx: number) {
    const i = this.editor.dv.getUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) +
        0x56 +
        folderIdx * 2
    );
    return i != 0xff ? i : null;
  }

  setTagChip1Index(folderIdx: number, i: number) {
    this.editor.dv.setUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) +
        0x56 +
        folderIdx * 2,
      i != null ? i : 0xff
    );
  }

  getTagChip2Index(folderIdx: number) {
    const i = this.editor.dv.getUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) +
        0x57 +
        folderIdx * 2
    );
    return i != 0xff ? i : null;
  }

  setTagChip2Index(folderIdx: number, i: number) {
    this.editor.dv.setUint8(
      this.editor.getNaviStatsOffset(this.editor.getCurrentNavi()) +
        0x57 +
        folderIdx * 2,
      i != null ? i : 0xff
    );
  }
}

class NavicustEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getNavicustProgramInfo(id: number) {
    return NCPS[id] ?? null;
  }

  getCommandLine() {
    return 3;
  }

  hasOutOfBounds() {
    return true;
  }

  getWidth() {
    return 7;
  }

  getHeight() {
    return 7;
  }

  getNavicustBlock(i: number) {
    const offset = this.editor.getNavicustOffset() + i * 8;
    const blockConstant = this.editor.dv.getUint8(offset);
    if (blockConstant == 0) {
      return null;
    }

    return {
      id: blockConstant >> 2,
      variant: blockConstant & 0x3,
      col: this.editor.dv.getUint8(offset + 3),
      row: this.editor.dv.getUint8(offset + 4),
      rot: this.editor.dv.getUint8(offset + 5),
      compressed: !!this.editor.dv.getUint8(offset + 6),
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
    const offset = this.editor.getNavicustOffset() + i * 8;
    this.editor.dv.setUint8(offset, (id << 2) | variant);
    this.editor.dv.setUint8(offset + 3, col);
    this.editor.dv.setUint8(offset + 4, row);
    this.editor.dv.setUint8(offset + 5, rot);
    this.editor.dv.setUint8(offset + 6, compressed ? 1 : 0);
    this.editor.navicustDirty = true;
  }
}

class ModcardsEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getModcardInfo(id: number) {
    return MODCARDS[id] ?? null;
  }

  getModcardCount() {
    return this.editor.dv.getUint8(0x65f0);
  }

  setModcardCount(n: number) {
    this.editor.dv.setUint8(0x65f0, n);
    this.editor.modcardsDirty = true;
  }

  getModcard(i: number) {
    if (i >= this.getModcardCount()) {
      return null;
    }

    const c = this.editor.dv.getUint8(0x6620 + i);
    return {
      id: c & 0x7f,
      enabled: !(c >> 7),
    };
  }

  setModcard(i: number, id: number, enabled: boolean) {
    this.editor.dv.setUint8(0x6620 + i, id | ((enabled ? 0 : 1) << 7));
    this.editor.modcardsDirty = true;
  }

  setModcardLoaded(id: number, loaded: boolean) {
    this.editor.dv.setUint8(
      0x5047 + id,
      this.editor.dv.getUint8(0x06bf + id) ^
        (loaded
          ? {
              falzar: 0x8d,
              gregar: 0x43,
            }[this.editor.getGameInfo().version]
          : 0xff)
    );
  }
}

export class Editor {
  dv: DataView;
  romViewer: ROMViewer;
  navicustDirty: boolean;
  modcardsDirty: boolean;

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

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, lang: string) {
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer, lang);

    this.navicustDirty = false;
    this.modcardsDirty = false;
  }

  getROMInfo() {
    return this.romViewer.getROMInfo();
  }

  getRawBufferForSave() {
    if (this.getChecksum() != this.computeChecksum()) {
      throw "checksum does not match";
    }
    if (this.navicustDirty) {
      throw "navicust must be rebuilt first";
    }
    if (this.modcardsDirty) {
      throw "modcards must be rebuilt first";
    }
    return this.dv.buffer;
  }

  rebuild() {
    this.rebuildNavicustTiles();
    this.rebuildModcardsLoaded();
    this.rebuildChecksum();
  }

  getChecksum() {
    return getChecksum(this.dv);
  }

  getGameInfo() {
    return GAME_INFOS[this.getROMInfo().name];
  }

  rebuildChecksum() {
    return this.dv.setUint32(CHECKSUM_OFFSET, this.computeChecksum(), true);
  }

  computeChecksum() {
    return computeChecksum(this.dv, this.getGameInfo().version);
  }

  getFolderEditor() {
    return new FolderEditor(this);
  }

  getNavicustEditor() {
    return new NavicustEditor(this);
  }

  getModcardsEditor() {
    if (this.getGameInfo().region != "JP") {
      return null;
    }
    return new ModcardsEditor(this);
  }

  getNaviStatsOffset(i: number) {
    return (
      (this.getGameInfo().region == "JP" ? 0x478c : 0x47cc) +
      0x64 * (i == 0 ? 0 : 1)
    );
  }

  getNavicustOffset() {
    return this.getGameInfo().region == "JP" ? 0x4150 : 0x4190;
  }

  getNavicustTilesOffset() {
    return this.getGameInfo().region == "JP" ? 0x410c : 0x414c;
  }

  rebuildNavicustTiles() {
    const navicustEditor = this.getNavicustEditor();

    const arr = new Uint8Array(
      this.dv.buffer,
      this.dv.byteOffset + this.getNavicustTilesOffset(),
      49
    );

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
          if (i2 >= 7 || j2 >= 7) {
            continue;
          }
          const v = squares[i * squares.ncols + j];
          if (v == 0) {
            continue;
          }
          if (placement.compressed && v != 1) {
            continue;
          }
          arr[i2 * 7 + j2] = idx + 1;
        }
      }
    }
    this.navicustDirty = false;
  }

  rebuildModcardsLoaded() {
    const modcardsEditor = this.getModcardsEditor();
    if (modcardsEditor == null) {
      return;
    }

    for (let i = 1; i < MODCARDS.length; ++i) {
      modcardsEditor.setModcardLoaded(i, false);
    }
    for (let i = 0; i < modcardsEditor.getModcardCount(); ++i) {
      modcardsEditor.setModcardLoaded(modcardsEditor.getModcard(i)!.id, true);
    }
    this.modcardsDirty = false;
  }

  getCurrentNavi() {
    return this.dv.getUint8(0x1b81);
  }

  setCurrentNavi(i: number) {
    this.dv.setUint8(0x1b81, i);
  }

  getRegMemory() {
    return this.dv.getUint8(
      this.getNaviStatsOffset(this.getCurrentNavi()) + 0x09
    );
  }

  getBaseHP() {
    return this.dv.getUint16(
      this.getNaviStatsOffset(this.getCurrentNavi()) + 0x3e,
      true
    );
  }

  getCurrentHP() {
    return this.dv.getUint16(
      this.getNaviStatsOffset(this.getCurrentNavi()) + 0x40,
      true
    );
  }

  getMaxHP() {
    return this.dv.getUint16(
      this.getNaviStatsOffset(this.getCurrentNavi()) + 0x42,
      true
    );
  }
}

interface ROMOffsets {
  chipData: number;
  chipIconPalettePointer: number;
  chipNamesPointers: number;
}

class ROMViewer extends ROMViewerBase {
  private offsets: ROMOffsets;
  private palette: Uint32Array;

  constructor(buffer: ArrayBuffer, lang: string) {
    super(buffer, lang);
    this.offsets = getOffsets(this.getROMInfo());
    this.palette = getPalette(
      this.dv,
      this.dv.getUint32(this.offsets.chipIconPalettePointer, true) & ~0x08000000
    );
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
    const element = this.dv.getUint8(dataOffset + 0x06);
    const class_ = this.dv.getUint8(dataOffset + 0x07);
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
      element: element.toString(),
      class: class_ == 1 ? "mega" : class_ == 2 ? "giga" : "standard",
      mb: this.dv.getUint8(dataOffset + 0x08),
      damage: (flags & 0x2) != 0 ? this.dv.getUint8(dataOffset + 0x1a) : 0,
    };
  }
}

function getOffsets(romInfo: ROMInfo): ROMOffsets {
  switch (romInfo.name) {
    case "ROCKEXE6_RXXBR6J":
    case "ROCKEXE6_GXXBR5J":
      return {
        chipData: 0x000221bc,
        chipIconPalettePointer: 0x0001f144,
        chipNamesPointers: 0x00028140,
      };
    case "MEGAMAN6_FXXBR6E":
    case "MEGAMAN6_GXXBR5E":
    case "MEGAMAN6_FXXBR6P":
    case "MEGAMAN6_GXXBR5P":
      return {
        chipData: 0x00021da8,
        chipIconPalettePointer: 0x0001ed20,
        chipNamesPointers: 0x00027d2c,
      };
  }
  throw `unknown rom: ${romInfo.name}`;
}

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
        case "\ue000":
          return "RV";
        case "\ue001":
          return "BX";
        case "\ue002":
          return "EX";
        case "\ue003":
          return "SP";
        case "\ue004":
          return "FZ";
      }
      return c;
    });
}
