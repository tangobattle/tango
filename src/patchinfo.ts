import { access, readdir, readFile, stat } from "fs/promises";
import path from "path";
import toml from "toml";
import semver from "semver";
import { constants } from "fs";

const decoder = new TextDecoder("utf-8");

export interface PatchInfos {
  [name: string]: PatchInfo;
}

export interface PatchVersionInfo {
  format: "ips" | "bps";
  netplayCompatibility: string;
}

export interface PatchInfo {
  title?: string;
  authors?: string[];
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

async function findAsync<T>(
  arr: Array<T>,
  asyncCallback: (v: T) => Promise<boolean>
) {
  const promises = arr.map(asyncCallback);
  const results = await Promise.all(promises);
  const index = results.findIndex((result) => result);
  return arr[index];
}

export async function scan(dir: string) {
  const patches = {} as {
    [name: string]: PatchInfo;
  };

  for (const patchName of await readdir(dir)) {
    const versions: { [version: string]: PatchInfo["versions"][""] } = {};

    const patchPath = path.join(dir, patchName);
    const statRes = await stat(patchPath);
    if (!statRes.isDirectory()) {
      continue;
    }

    let rawInfo;
    try {
      rawInfo = (await readFile(path.join(patchPath, "info.toml"))).buffer;
    } catch (e) {
      if ((e as any).code == "ENOENT") {
        continue;
      }
      console.warn(`could not scan patch info for ${patchName}: ${e}`);
    }

    let info;
    try {
      info = toml.parse(decoder.decode(rawInfo)) as RawPatchInfo;
    } catch (e) {
      console.warn(`could not parse patch info for ${patchName}: ${e}`);
      continue;
    }

    for (const versionName of Object.keys(info.versions)) {
      const version = info.versions[versionName];

      const parsedVersion = semver.parse(versionName);
      if (parsedVersion == null) {
        console.warn(
          `could not parse patch info for ${patchName}: could not parse version ${versionName}`
        );
        continue;
      }

      if (parsedVersion.format() != versionName) {
        console.warn(
          `could not parse patch info for ${patchName}: version ${versionName} did not roundtrip`
        );
        continue;
      }

      if (Object.prototype.hasOwnProperty.call(versions, versionName)) {
        console.warn(
          `could not parse patch info for ${patchName}: version already registered: ${JSON.stringify(
            versions[versionName]
          )}`
        );
        continue;
      }

      const format = await findAsync(["bps", "ips"], async (format) => {
        try {
          await access(
            path.join(patchPath, `v${versionName}.${format}`),
            constants.R_OK
          );
        } catch (e) {
          return false;
        }
        return true;
      });

      if (format == null) {
        console.warn(
          `could not find patch file for ${patchName} at version ${versionName}`
        );
        continue;
      }

      versions[versionName] = {
        format: format as any,
        netplayCompatibility: version.netplay_compatibility,
      };
    }

    patches[patchName] = {
      title: info.patch.title || patchName,
      authors: info.patch.authors || [],
      source: info.patch.source,
      license: info.patch.license,
      forROM: info.patch.for_rom,
      versions,
    };
  }

  return patches;
}

export function findPatchVersion(info: PatchInfo, requirement: string) {
  return semver.maxSatisfying(Object.keys(info.versions), requirement);
}
