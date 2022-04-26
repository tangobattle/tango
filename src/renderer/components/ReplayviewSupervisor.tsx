export default function ReplayviewSupervisor({
  romPath,
  replayPath,
  patchPath,
  onExit,
}: {
  romPath: string;
  replayPath: string;
  patchPath?: string;
  onExit: () => void;
}) {
  console.log(romPath, replayPath, patchPath);
  return null;
}
