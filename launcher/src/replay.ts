import { open } from "fs/promises";

import { ReplayMetadata } from "./protos/generated/replay";

const REPLAY_VERSION = 0x10;

export interface ReplayInfo {
  metadata: ReplayMetadata;
  isComplete: boolean;
}

export async function readReplayMetadata(
  filename: string
): Promise<ReplayInfo | null> {
  const fd = await open(filename, "r");
  let i = 0;
  try {
    {
      const chunks = [];
      for await (const chunk of fd.createReadStream({
        start: i,
        end: i + 5 - 1,
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
        console.warn("replay skipped:", filename, "invalid header");
        return null;
      }
    }
    i += 5;

    let isComplete;
    {
      const chunks = [];
      for await (const chunk of fd.createReadStream({
        start: i,
        end: i + 4 - 1,
        autoClose: false,
      })) {
        chunks.push(chunk);
      }
      const numInputs = new DataView(
        new Uint8Array(Buffer.concat(chunks)).buffer
      ).getUint32(0, true);
      isComplete = numInputs != 0;
    }
    i += 4;

    let metaSize = 0;
    {
      const chunks = [];
      for await (const chunk of fd.createReadStream({
        start: i,
        end: i + 4 - 1,
        autoClose: false,
      })) {
        chunks.push(chunk);
      }
      metaSize = new DataView(
        new Uint8Array(Buffer.concat(chunks)).buffer
      ).getUint32(0, true);
    }
    i += 4;

    const chunks = [];
    for await (const chunk of fd.createReadStream({
      start: i,
      end: i + metaSize - 1,
      autoClose: false,
    })) {
      chunks.push(chunk);
    }
    return {
      isComplete,
      metadata: ReplayMetadata.decode(new Uint8Array(Buffer.concat(chunks))),
    };
  } catch (e) {
    console.warn("replay skipped:", filename, e);
    return null;
  } finally {
    await fd.close();
  }
}
