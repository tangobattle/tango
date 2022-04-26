export default function ReplaydumpSupervisor({
  romPath,
  replayPath,
  patchPath,
  outPath,
  onExit,
}: {
  romPath: string;
  replayPath: string;
  patchPath?: string;
  outPath: string;
  onExit: () => void;
}) {
  console.log(romPath, replayPath, patchPath, outPath);
  return null;
}
