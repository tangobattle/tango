import type { Chip, Modcard, NavicustProgram } from "../";
import array2d from "../../array2d";
import { EditorBase } from "../base";
import {
    ByteReader, getChipText, getPalette, getTextSimple, getTiles, NewlineControl, parseText,
    ParseText1, ROMViewerBase, unlz77
} from "../rom";

const CHIP_CODES = "ABCDEFGHIJKLMNOPQRSTUVWXYZ*";

const SRAM_START_OFFSET = 0x0100;
const SRAM_SIZE = 0x6710;
const MASK_OFFSET = 0x1064;
const GAME_NAME_OFFSET = 0x1c70;
const CHECKSUM_OFFSET = 0x1c6c;

type Control = NewlineControl | { c: "print"; v: number };

function parseText1(br: ByteReader): ReturnType<ParseText1<Control>> {
  const b = br.readByte();
  switch (b) {
    case 0xe4:
      return { t: 0xe4 + br.readByte() };
    case 0xe6:
      return null;
    case 0xe9:
      return { c: "newline" };
    case 0xfa: {
      br.readByte();
      br.readByte();
      const v = br.readByte();
      return { c: "print", v };
    }
  }
  return { t: b };
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
    const fullID = this.editor.dv.getUint16(
      0x2178 + folderIdx * (30 * 2) + chipIdx * 2,
      true
    );

    if (fullID == 0) {
      return null;
    }

    return {
      id: fullID & 0x1ff,
      variant: fullID >> 9,
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

  getElementIcons() {
    return this.editor.romViewer.getElementIcons();
  }
}

class NavicustEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getNavicustTiles() {
    return Array.from(
      new Uint8Array(
        this.editor.dv.buffer,
        this.editor.getNavicustTilesOffset(),
        49
      )
    );
  }

  getNavicustProgramInfo(id: number, variant: number) {
    return this.editor.romViewer.getNavicustProgramInfo(id, variant);
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

  getStyle() {
    return null;
  }

  getStyleInfo(_id: number) {
    return null;
  }

  getNavicustBlock(i: number) {
    const offset = this.editor.getNavicustOffset() + i * 8;
    const fullID = this.editor.dv.getUint8(offset);
    if (fullID == 0) {
      return null;
    }

    return {
      id: fullID >> 2,
      variant: fullID & 0x3,
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
    return this.editor.romViewer.getModcardInfo(id)!;
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

export class Editor extends EditorBase {
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

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, saveeditInfo: any) {
    super();
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer, this.dv, saveeditInfo);

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

    for (let idx = 0; idx < 25; ++idx) {
      const placement = navicustEditor.getNavicustBlock(idx);
      if (placement == null) {
        continue;
      }

      const ncp = this.romViewer.getNavicustProgramInfo(
        placement.id,
        placement.variant
      );

      let squares = placement.compressed ? ncp.compressed : ncp.uncompressed;
      for (let i = 0; i < placement.rot; ++i) {
        squares = array2d.rot90(squares);
      }

      for (let i = 0; i < squares.nrows; ++i) {
        for (let j = 0; j < squares.ncols; ++j) {
          const i2 = i + placement.row - 3;
          const j2 = j + placement.col - 3;
          if (i2 >= 7 || j2 >= 7) {
            continue;
          }
          const v = squares[i * squares.ncols + j];
          if (!v) {
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

    for (let i = 1; i < 118; ++i) {
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

interface SaveeditInfo {
  charset: string[];
  offsets: {
    chipData: number;
    chipIconPalettePointer: number;
    chipNamesPointers: number;
    ncpData: number;
    ncpNamesPointer: number;
    elementIconPalettePointer: number;
    elementIconsPointer: number;
    modcardData: number | null;
    modcardNamesPointer: number | null;
    modcardDetailsNamesPointer: number | null;
  };
  strings: {
    chips: string[] | null;
    ncps: string[] | null;
    modcards: string[] | null;
    modcardEffects: Chunk[][] | null;
  } | null;
}

class ROMViewer extends ROMViewerBase {
  private chipIconPalette: Uint32Array;
  private elementIconPalette: Uint32Array;
  private saveDv: DataView;
  private saveeditInfo: SaveeditInfo;
  private modcardTextArchive: ArrayBuffer | null;
  private modcardDetailsTextArchive: ArrayBuffer | null;

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
      this.dv.getUint32(saveeditInfo.offsets.chipIconPalettePointer, true) &
        ~0x08000000
    );
    this.elementIconPalette = getPalette(
      this.dv,
      this.dv.getUint32(saveeditInfo.offsets.elementIconPalettePointer, true) &
        ~0x08000000
    );
    this.modcardTextArchive =
      saveeditInfo.offsets.modcardNamesPointer != null
        ? unlz77(
            new DataView(
              buffer,
              this.dv.getUint32(
                saveeditInfo.offsets.modcardNamesPointer,
                true
              ) & ~0x88000000
            )
          )
        : null;
    this.modcardDetailsTextArchive =
      saveeditInfo.offsets.modcardDetailsNamesPointer != null
        ? unlz77(
            new DataView(
              buffer,
              this.dv.getUint32(
                saveeditInfo.offsets.modcardDetailsNamesPointer,
                true
              ) & ~0x88000000
            )
          )
        : null;
  }

  getElementIcons(): ImageData[] {
    const icons: ImageData[] = [];
    const start =
      this.dv.getUint32(this.saveeditInfo.offsets.elementIconsPointer, true) &
      ~0x08000000;
    for (let i = 0; i < 11; ++i) {
      icons.push(
        getTiles(this.dv, this.elementIconPalette, start + i * 0x80, 2, 2)
      );
    }
    return icons;
  }

  getModcardInfo(id: number): Modcard | null {
    if (
      this.saveeditInfo.offsets.modcardData == null ||
      this.modcardTextArchive == null ||
      this.modcardDetailsTextArchive == null
    ) {
      return null;
    }

    const modcardStart = this.dv.getUint16(
      this.saveeditInfo.offsets.modcardData + id * 2,
      true
    );

    const modcardEnd = this.dv.getUint16(
      this.saveeditInfo.offsets.modcardData + (id + 1) * 2,
      true
    );

    const detailsDv = new DataView(this.modcardDetailsTextArchive);

    const effects = [];

    for (let offset = modcardStart + 3; offset < modcardEnd; offset += 3) {
      const id = this.dv.getUint8(
        this.saveeditInfo.offsets.modcardData + offset
      );
      const parameter = this.dv.getUint8(
        this.saveeditInfo.offsets.modcardData + offset + 1
      );
      const debuff = !!this.dv.getUint8(
        this.saveeditInfo.offsets.modcardData + offset + 2
      );

      const tmpl =
        this.saveeditInfo.strings == null ||
        this.saveeditInfo.strings.modcardEffects == null
          ? parseText(detailsDv, 4, id, parseText1).flatMap<
              { t: string } | { p: number }
            >((chunk) => {
              if ("t" in chunk) {
                return [
                  {
                    t: this.saveeditInfo.charset[chunk.t],
                  },
                ];
              }
              if ("c" in chunk) {
                switch (chunk.c) {
                  case "print":
                    return [{ p: chunk.v }];
                  case "newline":
                    return [{ t: "\n" }];
                }
              }
              return [];
            })
          : this.saveeditInfo.strings.modcardEffects[id];

      effects.push({
        id,
        name: tmpl
          .map((chunk) => {
            if ("t" in chunk) {
              return chunk.t;
            }
            if ("p" in chunk) {
              if (chunk.p == 1) {
                let p = parameter;
                if (id == 0x00 || id == 0x02) {
                  p = p * 10;
                }
                return p.toString();
              }
              return "";
            }
            return "";
          })
          .join(""),
        parameter,
        isAbility: id > 0x15,
        debuff,
      });
    }

    return {
      name:
        this.saveeditInfo.strings == null ||
        this.saveeditInfo.strings.modcards == null
          ? getTextSimple(
              new DataView(this.modcardTextArchive),
              4,
              id,
              this.saveeditInfo.charset,
              parseText1
            )
          : this.saveeditInfo.strings.modcards[id],
      mb: this.dv.getUint8(
        this.saveeditInfo.offsets.modcardData + modcardStart + 0x01
      ),
      effects,
    };
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

    const damage = this.dv.getUint16(dataOffset + 0x1a, true);
    const iconPtr = this.dv.getUint32(dataOffset + 0x20, true);

    return {
      name:
        this.saveeditInfo.strings == null ||
        this.saveeditInfo.strings.chips == null
          ? getChipText(
              this.dv,
              this.saveeditInfo.offsets.chipNamesPointers,
              id,
              this.saveeditInfo.charset,
              parseText1
            )
          : this.saveeditInfo.strings.chips[id],
      codes: codes.join(""),
      icon:
        iconPtr >= 0x08000000
          ? getTiles(this.dv, this.chipIconPalette, iconPtr & ~0x08000000, 2, 2)
          : getTiles(
              this.saveDv,
              this.chipIconPalette,
              iconPtr & ~0x02000000,
              2,
              2
            ),

      element: this.dv.getUint8(dataOffset + 0x06),
      class: ["standard", "mega", "giga", null, "pa"][
        this.dv.getUint8(dataOffset + 0x07)
      ] as Chip["class"],
      dark: false,
      mb: this.dv.getUint8(dataOffset + 0x08),
      damage: damage < 1000 ? damage : 0,
    };
  }

  getNavicustProgramInfo(id: number, variant: number): NavicustProgram {
    const dataOffset = this.saveeditInfo.offsets.ncpData + id * 0x40;

    const subdataOffset = dataOffset + variant * 0x10;

    return {
      name:
        this.saveeditInfo.strings == null ||
        this.saveeditInfo.strings.ncps == null
          ? getTextSimple(
              this.dv,
              this.dv.getUint32(
                this.saveeditInfo.offsets.ncpNamesPointer,
                true
              ) & ~0x08000000,
              id,
              this.saveeditInfo.charset,
              parseText1
            )
          : this.saveeditInfo.strings.ncps[id],
      color: [null, "white", "yellow", "pink", "red", "blue", "green"][
        this.dv.getUint8(subdataOffset + 0x3)
      ] as NavicustProgram["color"],
      isSolid: this.dv.getUint8(subdataOffset + 0x1) == 0,
      uncompressed: array2d.from(
        [
          ...new Uint8Array(
            this.dv.buffer,
            this.dv.getUint32(subdataOffset + 0x8, true) & ~0x08000000,
            7 * 7
          ),
        ].map((v) => !!v),
        7,
        7
      ),
      compressed: array2d.from(
        [
          ...new Uint8Array(
            this.dv.buffer,
            this.dv.getUint32(subdataOffset + 0xc, true) & ~0x08000000,
            7 * 7
          ),
        ].map((v) => !!v),
        7,
        7
      ),
    };
  }
}

type Chunk = { t: string } | { p: number };
