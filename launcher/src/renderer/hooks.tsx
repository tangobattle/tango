import path from "path";

import { app } from "@electron/remote";

import { getPatchesPath, getROMsPath } from "../paths";
import { useROMs } from "./components/ROMsContext";

export function useGetROMPath() {
  const { roms } = useROMs();
  return (romName: string) =>
    path.join(getROMsPath(app), roms[romName].filename);
}

export function useGetPatchPath() {
  const { roms } = useROMs();

  return (rom: string, patch: { name: string; version: string }) =>
    path.join(
      getPatchesPath(app),
      patch.name,
      `v${patch.version}`,
      `${rom}_${roms[rom].revision.toString().padStart(2, "0")}.bps`
    );
}
