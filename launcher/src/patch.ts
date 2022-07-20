import { parseOneAddress } from "email-addresses";
import fs from "fs";
import { readdir, readFile, stat } from "fs/promises";
import * as git from "isomorphic-git";
import * as http from "isomorphic-git/http/node";
import mkdirp from "mkdirp";
import path from "path";
import semver from "semver";
import toml from "toml";

const decoder = new TextDecoder("utf-8");

export interface PatchInfos {
  [name: string]: PatchInfo;
}

export interface PatchVersionInfo {
  netplayCompatibility: string;
  saveeditOverrides?: any;
  forROMs: {
    name: string;
    revision: number;
  }[];
}

export interface PatchInfo {
  title: string;
  readme: string | null;
  authors: { name: string; email: string | null }[];
  source?: string;
  license?: string;
  lang?: string;
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
    lang?: string;
  };
  versions: {
    [version: string]: {
      netplay_compatibility: string;
      saveedit_overrides?: any;
    };
  };
}

export async function update(dir: string, url: string) {
  try {
    await git.addRemote({ fs, dir, remote: "origin", url, force: true });
  } catch (e) {
    // eslint-disable-next-line no-console
    console.info(
      "did not manage to add remote, we probably don't have a repo here",
      e
    );
    await git.init({ fs, dir });
    await git.addRemote({ fs, dir, remote: "origin", url, force: true });
  }
  await git.fetch({
    fs,
    http,
    dir,
    remote: "origin",
    ref: "main",
  });
  await git.checkout({ fs, dir, ref: "remotes/origin/main", force: true });
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

        const filenames = await readdir(patchPath);
        const readmeFilename = filenames.find(
          (f) => f.toLowerCase() == "readme"
        );
        const readme =
          readmeFilename != null
            ? (await readFile(path.join(patchPath, readmeFilename))).toString()
            : null;

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

          let patchFiles: string[];
          try {
            patchFiles = await readdir(path.join(patchPath, `v${versionName}`));
          } catch (e) {
            throw `could not find patch folder for ${patchName} at version ${versionName}`;
          }

          versions[versionName] = {
            netplayCompatibility: version.netplay_compatibility,
            saveeditOverrides: version.saveedit_overrides,
            forROMs: patchFiles.flatMap((pf) => {
              if (path.extname(pf) != ".bps") {
                return [];
              }
              const fullRomName = path.basename(pf, ".bps");
              const delimIdx = fullRomName.lastIndexOf("_");
              const revision = parseInt(
                fullRomName.substring(delimIdx + 1),
                10
              );
              return [
                {
                  name: fullRomName.substring(0, delimIdx).replace(/@/g, "\0"),
                  revision,
                },
              ];
            }),
          };
        }

        patches[patchName] = {
          title: info.patch.title || patchName,
          readme,
          authors:
            info.patch.authors != null
              ? info.patch.authors.flatMap((a) => {
                  const addr = parseOneAddress(a);
                  if (addr == null || addr.type != "mailbox") {
                    return [{ name: a, email: null as string | null }];
                  }
                  return [
                    {
                      name: addr.name ?? addr.address,
                      email: addr.address as string | null,
                    },
                  ];
                })
              : [],
          lang: info.patch.lang,
          source: info.patch.source,
          license: info.patch.license,
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
