import array2d from "../../array2d";
import { EditorBase } from "../base";
import {
    ByteReader, getPalette, getTextSimple, getTiles, NewlineControl, parseText, ParseText1,
    ROMViewerBase, unlz77
} from "../rom";

import type { Chip, NavicustProgram, Modcard } from "../";
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

type Control =
  | NewlineControl
  | { c: "print"; v: number }
  | { c: "ereader"; v: number };

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
    case 0xff: {
      br.readByte();
      const v = br.readByte();
      return { c: "ereader", v };
    }
  }
  return { t: b };
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

  getNavicustProgramInfo(id: number, variant: number) {
    return this.editor.romViewer.getNavicustProgramInfo(id, variant);
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

  getStyle() {
    return null;
  }

  getStyleInfo(_id: number) {
    return null;
  }

  getNavicustTiles() {
    return Array.from(
      new Uint8Array(
        this.editor.dv.buffer,
        this.editor.dv.byteOffset + 0x4d48,
        25
      )
    );
  }

  getNavicustBlock(i: number) {
    const offset = 0x4d6c + i * 8;
    const fullID = this.editor.dv.getUint8(offset);
    if (fullID == 0) {
      return null;
    }

    return {
      id: fullID >> 2,
      variant: fullID & 0x3,
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
    this.editor.dv.setUint8(offset + 2, col);
    this.editor.dv.setUint8(offset + 3, row);
    this.editor.dv.setUint8(offset + 4, rot);
    this.editor.dv.setUint8(offset + 5, compressed ? 1 : 0);
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
    const fullID = this.editor.dv.getUint16(
      0x2df4 + folderIdx * (30 * 2) + chipIdx * 2,
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
      0x2df4 + folderIdx * (30 * 2) + chipIdx * 2,
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

class ModcardsEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getModcardInfo(id: number) {
    return this.editor.romViewer.getModcardInfo(id)!;
  }

  getModcardCount() {
    return this.editor.dv.getUint8(0x79a0);
  }

  getModcard(i: number) {
    if (i >= this.getModcardCount()) {
      return null;
    }

    const c = this.editor.dv.getUint8(0x79d0 + i);
    return {
      id: c & 0x7f,
      enabled: !(c >> 7),
    };
  }
}

class DarkAIEditor {
  private editor: Editor;

  constructor(editor: Editor) {
    this.editor = editor;
  }

  getNumChips() {
    return 368;
  }

  getChipUseCount(id: number) {
    return this.editor.dv.getUint16(0x7340 + id * 2, true);
  }

  getSecondaryChipUseCount(id: number) {
    return this.editor.dv.getUint16(0x2340 + id * 2, true);
  }

  getNumSlots() {
    return 0x2a;
  }

  getSlot(i: number): { type: "chip" | "combo"; id: number } | null {
    if (i >= this.getNumSlots()) {
      return null;
    }

    const id = this.editor.dv.getUint16(0x554c + i * 2, true);
    if (id == 0xffff) {
      return null;
    }

    return (id & 0x8000) != 0
      ? { type: "combo", id: id & 0x7fff }
      : { type: "chip", id };
  }
}

export class Editor extends EditorBase {
  dv: DataView;
  romViewer: ROMViewer;
  navicustDirty: boolean;

  constructor(buffer: ArrayBuffer, romBuffer: ArrayBuffer, saveeditInfo: any) {
    super();
    this.dv = new DataView(buffer);
    this.romViewer = new ROMViewer(romBuffer, this.dv, saveeditInfo);
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
        for (let j = 0; j < squares.nrows; ++j) {
          const i2 = i + placement.row - 2;
          const j2 = j + placement.col - 2;
          if (i2 >= 5 || j2 >= 5) {
            continue;
          }
          const v = squares[i * squares.ncols + j];
          if (!v) {
            continue;
          }
          arr[i2 * 5 + j2] = idx + 1;
        }
      }
    }
    this.navicustDirty = false;
  }

  rebuild() {
    this.rebuildNavicustTiles();
    this.rebuildChecksum();
  }

  getChecksum() {
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
    return new ModcardsEditor(this);
  }

  getDarkAIEditor() {
    return new DarkAIEditor(this);
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
    modcardData: number;
    modcardNamesPointer: number;
    modcardDetailsNamesPointer: number;
  };
}

class ROMViewer extends ROMViewerBase {
  private chipIconPalette: Uint32Array;
  private elementIconPalette: Uint32Array;
  private saveDv: DataView;
  private saveeditInfo: SaveeditInfo;
  private modcardTextArchive: ArrayBuffer;
  private modcardDetailsTextArchive: ArrayBuffer;

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
    this.modcardTextArchive = unlz77(
      new DataView(
        buffer,
        this.dv.getUint32(saveeditInfo.offsets.modcardNamesPointer, true) &
          ~0x88000000
      )
    );
    this.modcardDetailsTextArchive = unlz77(
      new DataView(
        buffer,
        this.dv.getUint32(
          saveeditInfo.offsets.modcardDetailsNamesPointer,
          true
        ) & ~0x88000000
      )
    );
  }

  getElementIcons(): ImageData[] {
    const icons: ImageData[] = [];
    const start =
      this.dv.getUint32(this.saveeditInfo.offsets.elementIconsPointer, true) &
      ~0x08000000;
    for (let i = 0; i < 13; ++i) {
      icons.push(
        getTiles(this.dv, this.elementIconPalette, start + i * 0x80, 2, 2)
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
    const damage = this.dv.getUint16(dataOffset + 0x1a, true);
    const iconPtr = this.dv.getUint32(dataOffset + 0x20, true);

    return {
      name: (() => {
        let scriptEntryID = id;
        let scriptPointerOffset = this.saveeditInfo.offsets.chipNamesPointers;
        if (scriptEntryID > 0xff) {
          scriptPointerOffset += 4;
          scriptEntryID -= 0x100;
        }
        return parseText(
          this.dv,
          this.dv.getUint32(scriptPointerOffset, true) & ~0x08000000,
          scriptEntryID,
          parseText1
        )
          .flatMap((chunk) => {
            if ("t" in chunk) {
              return this.saveeditInfo.charset[chunk.t];
            }

            if ("c" in chunk) {
              switch (chunk.c) {
                case "newline":
                  return ["\n"];
                case "ereader":
                  return [
                    getTextSimple(
                      this.saveDv,
                      0x1d14 + chunk.v * 0x18,
                      0,
                      this.saveeditInfo.charset,
                      parseText1
                    ),
                  ];
              }
            }
            return [];
          })
          .join("")
          .replace(/-\n/g, "-")
          .replace(/\n/g, " ");
      })(),
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
      dark: (flags & 0x20) != 0,
      mb: this.dv.getUint8(dataOffset + 0x08),
      damage: damage < 1000 ? damage : 0,
    };
  }

  getNavicustProgramInfo(id: number, variant: number): NavicustProgram {
    const dataOffset = this.saveeditInfo.offsets.ncpData + id * 0x40;

    const subdataOffset = dataOffset + variant * 0x10;

    return {
      name: getTextSimple(
        this.dv,
        this.dv.getUint32(this.saveeditInfo.offsets.ncpNamesPointer, true) &
          ~0x08000000,
        id,
        this.saveeditInfo.charset,
        parseText1
      ),
      color: [null, "white", "yellow", "pink", "red", "blue", "green"][
        this.dv.getUint8(subdataOffset + 0x3)
      ] as NavicustProgram["color"],
      isSolid: this.dv.getUint8(subdataOffset + 0x1) == 0,
      uncompressed: array2d.from(
        [
          ...new Uint8Array(
            this.dv.buffer,
            this.dv.getUint32(subdataOffset + 0x8, true) & ~0x08000000,
            5 * 5
          ),
        ].map((v) => !!v),
        5,
        5
      ),
      compressed: array2d.from(
        [
          ...new Uint8Array(
            this.dv.buffer,
            this.dv.getUint32(subdataOffset + 0xc, true) & ~0x08000000,
            5 * 5
          ),
        ].map((v) => !!v),
        5,
        5
      ),
    };
  }

  getModcardInfo(id: number): Modcard | null {
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

      const tmpl = parseText(detailsDv, 4, id, parseText1).flatMap<
        { t: string } | { p: number }
      >((chunk) => {
        if ("t" in chunk) {
          return [{ t: this.saveeditInfo.charset[chunk.t] }];
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
      });

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
      name: getTextSimple(
        new DataView(this.modcardTextArchive),
        4,
        id,
        this.saveeditInfo.charset,
        parseText1
      ),
      mb: this.dv.getUint8(
        this.saveeditInfo.offsets.modcardData + modcardStart + 0x01
      ),
      effects,
    };
  }
}
