export interface ReplayInfo {
  ts: number;
  rom: string;
  patch: {
    name: string;
    version: string;
  } | null;
}
