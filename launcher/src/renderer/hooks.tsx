import path from "path";

import { app } from "@electron/remote";

import { getPatchesPath, getROMsPath } from "../paths";
import { useROMs } from "./components/ROMsContext";

export function useROMPath(romName: string | null) {
  const { roms } = useROMs();
  if (romName == null) {
    return null;
  }
  return path.join(getROMsPath(app), roms[romName]);
}

export function usePatchPath(patch: { name: string; version: string } | null) {
  if (patch == null) {
    return null;
  }
  return path.join(getPatchesPath(app), patch.name, `v${patch.version}.bps`);
}
