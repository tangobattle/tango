export interface ReplayInfo {
  rom: string;
  patch: {
    name: string;
    version: string;
  } | null;
}
