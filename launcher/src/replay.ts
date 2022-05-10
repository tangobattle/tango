import { open } from "fs/promises";

const REPLAY_VERSION = 0x0f;

export interface GameInfo {
  rom: string;
  patch: {
    name: string;
    version: string;
  } | null;
}

export interface ReplayInfo extends GameInfo {
  ts: number;
  linkCode: string;
  remote: (GameInfo & { nickname: string }) | null;
}

const textDecoder = new TextDecoder("utf-8");

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
      if (numInputs == 0) {
        console.warn("replay skipped:", filename, "incomplete");
        return null;
      }
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
    return JSON.parse(
      textDecoder.decode(new Uint8Array(Buffer.concat(chunks)).buffer)
    );
  } catch (e) {
    console.warn("replay skipped:", filename, e);
    return null;
  } finally {
    await fd.close();
  }
}
