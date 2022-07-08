import path from "path";

import { useConfig } from "./components/ConfigContext";
import { useROMs } from "./components/ROMsContext";

export function useGetROMPath() {
  const { roms } = useROMs();
  const { config } = useConfig();
  return (romName: string) =>
    path.join(config.paths.roms, roms[romName].filename);
}

export function useGetPatchPath() {
  const { roms } = useROMs();
  const { config } = useConfig();

  return (rom: string, patch: { name: string; version: string }) =>
    path.join(
      config.paths.patches,
      patch.name,
      `v${patch.version}`,
      `${rom.replace(/\0/g, "@")}_${roms[rom].revision
        .toString()
        .padStart(2, "0")}.bps`
    );
}
