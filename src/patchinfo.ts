import { parseOneAddress } from "email-addresses";
import { constants } from "fs";
import { access, readdir, readFile, stat } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";
import semver from "semver";
import toml from "toml";

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

  let patchNames: string[];
  try {
    patchNames = await readdir(dir);
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      await mkdirp(dir);
      patchNames = [];
    } else {
      throw e;
    }
  }

  for (const result of await Promise.allSettled(
    patchNames.map(async (patchName) => {
      try {
        const versions: { [version: string]: PatchInfo["versions"][""] } = {};

        const patchPath = path.join(dir, patchName);
        const statRes = await stat(patchPath);
        if (!statRes.isDirectory()) {
          return;
        }

        let rawInfo;
        try {
          rawInfo = (await readFile(path.join(patchPath, "info.toml"))).buffer;
        } catch (e) {
          if ((e as any).code == "ENOENT") {
            return;
          }
          throw `could not scan patch info for ${patchName}: ${e}`;
        }

        let info;
        try {
          info = toml.parse(decoder.decode(rawInfo)) as RawPatchInfo;
        } catch (e) {
          throw `could not parse patch info for ${patchName}: ${e}`;
        }

        for (const versionName of Object.keys(info.versions)) {
          const version = info.versions[versionName];

          const parsedVersion = semver.parse(versionName);
          if (parsedVersion == null) {
            throw `could not parse patch info for ${patchName}: could not parse version ${versionName}`;
          }

          if (parsedVersion.format() != versionName) {
            throw `could not parse patch info for ${patchName}: version ${versionName} did not roundtrip`;
          }

          if (Object.prototype.hasOwnProperty.call(versions, versionName)) {
            throw `could not parse patch info for ${patchName}: version already registered: ${JSON.stringify(
              versions[versionName]
            )}`;
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
            throw `could not find patch file for ${patchName} at version ${versionName}`;
          }

          versions[versionName] = {
            format: format as any,
            netplayCompatibility: version.netplay_compatibility,
          };
        }

        patches[patchName] = {
          title: info.patch.title || patchName,
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
      } catch (e) {
        throw `failed to scan patch ${patchName}: ${e}`;
      }
    })
  )) {
    if (result.status == "rejected") {
      console.warn("patch skipped:", result.reason);
    }
  }

  return patches;
}

export function findPatchVersion(info: PatchInfo, requirement: string) {
  return semver.maxSatisfying(Object.keys(info.versions), `~${requirement}`);
}
