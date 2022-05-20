import { parseOneAddress } from "email-addresses";
import { constants } from "fs";
import { access, readFile, stat } from "fs/promises";
import path from "path";
import semver from "semver";
import toml from "toml";

const decoder = new TextDecoder("utf-8");

export interface PatchInfos {
  [name: string]: PatchInfo;
}

export interface PatchVersionInfo {
  netplayCompatibility: string;
}

export interface PatchInfo {
  title?: string;
  authors: { name: string | null; email: string }[];
  source?: string;
  license?: string;
  forROM: string;
  versions: {
    [version: string]: PatchVersionInfo;
  };
}

interface RawPatchInfo {
  patch: {
    title: string;
    authors: string[];
    source?: string;
    license?: string;
    for_rom: string;
  };
  versions: {
    [version: string]: {
      netplay_compatibility: string;
    };
  };
}

export async function getPatchInfo(dir: string): Promise<PatchInfo | null> {
  const versions: { [version: string]: PatchInfo["versions"][""] } = {};

  const statRes = await stat(dir);
  if (!statRes.isDirectory()) {
    return null;
  }

  let rawInfo;
  try {
    rawInfo = (await readFile(path.join(dir, "info.toml"))).buffer;
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      return null;
    }
    throw `could not scan patch info for ${path}: ${e}`;
  }

  let info;
  try {
    info = toml.parse(decoder.decode(rawInfo)) as RawPatchInfo;
  } catch (e) {
    throw `could not parse patch info for ${path}: ${e}`;
  }

  for (const versionName of Object.keys(info.versions)) {
    const version = info.versions[versionName];

    const parsedVersion = semver.parse(versionName);
    if (parsedVersion == null) {
      throw `could not parse patch info for ${path}: could not parse version ${versionName}`;
    }

    if (parsedVersion.format() != versionName) {
      throw `could not parse patch info for ${path}: version ${versionName} did not roundtrip`;
    }

    if (Object.prototype.hasOwnProperty.call(versions, versionName)) {
      throw `could not parse patch info for ${path}: version already registered: ${JSON.stringify(
        versions[versionName]
      )}`;
    }

    try {
      await access(path.join(dir, `v${versionName}.bps`), constants.R_OK);
    } catch (e) {
      throw `could not find patch file for ${path} at version ${versionName}`;
    }

    versions[versionName] = {
      netplayCompatibility: version.netplay_compatibility,
    };
  }

  return {
    title: info.patch.title || path.basename(dir),
    authors:
      info.patch.authors != null
        ? info.patch.authors.flatMap((a) => {
            const addr = parseOneAddress(a);
            if (addr == null || addr.type != "mailbox") {
              return [];
            }
            return [{ name: addr.name, email: addr.address }];
          })
        : [],
    source: info.patch.source,
    license: info.patch.license,
    forROM: info.patch.for_rom,
    versions,
  };
}

export function findPatchVersion(info: PatchInfo, requirement: string) {
  return semver.maxSatisfying(Object.keys(info.versions), `~${requirement}`);
}
