import { open } from "fs/promises";

const REPLAY_VERSION = 0x0d;

export interface ReplayInfo {
  ts: number;
  rom: string;
  patch: {
    name: string;
    version: string;
  } | null;
}

const textDecoder = new TextDecoder("utf-8");

export async function readReplayMetadata(
  filename: string
): Promise<ReplayInfo | null> {
  const fd = await open(filename, "r");
  try {
    {
      const chunks = [];
      for await (const chunk of fd.createReadStream({
        start: 0,
        end: 0 + 4,
        autoClose: false,
      })) {
        chunks.push(chunk);
      }
      const header = Buffer.concat(chunks);
      if (
        header[0] != 84 /* T */ ||
        header[1] != 79 /* O */ ||
        header[2] != 79 /* O */ ||
        header[3] != 84 /* T */ ||
        header[4] != REPLAY_VERSION
      ) {
        return null;
      }
    }

    let metaSize = 0;
    {
      const chunks = [];
      for await (const chunk of fd.createReadStream({
        start: 5,
        end: 5 + 4 - 1,
        autoClose: false,
      })) {
        chunks.push(chunk);
      }
      metaSize = new DataView(
        new Uint8Array(Buffer.concat(chunks)).buffer
      ).getUint32(0, true);
    }

    const chunks = [];
    for await (const chunk of fd.createReadStream({
      start: 5 + 4,
      end: 5 + 4 + metaSize - 1,
      autoClose: false,
    })) {
      chunks.push(chunk);
    }
    return JSON.parse(
      textDecoder.decode(new Uint8Array(Buffer.concat(chunks)).buffer)
    );
  } finally {
    await fd.close();
  }
}
