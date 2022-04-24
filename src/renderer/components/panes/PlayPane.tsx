import { Trans, useTranslation } from "react-i18next";
import React from "react";
import semver from "semver";
import { useROMs } from "../ROMsContext";
import { usePatches } from "../PatchesContext";
import { KNOWN_ROMS } from "../../../rom";
import { ParsedMailbox, parseOneAddress } from "email-addresses";
import { CoreSupervisor } from "../CoreSupervisor";
import Select from "@mui/material/Select";
import ListSubheader from "@mui/material/ListSubheader";
import InputAdornment from "@mui/material/InputAdornment";
import IconButton from "@mui/material/IconButton";
import MenuItem from "@mui/material/MenuItem";
import ListItemText from "@mui/material/ListItemText";
import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import FormControl from "@mui/material/FormControl";
import TextField from "@mui/material/TextField";
import InputLabel from "@mui/material/InputLabel";
import Link from "@mui/material/Link";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import Tooltip from "@mui/material/Tooltip";
import SportsMmaIcon from "@mui/icons-material/SportsMma";
import SportsEsportsOutlinedIcon from "@mui/icons-material/SportsEsportsOutlined";
import RefreshIcon from "@mui/icons-material/Refresh";
import { CopyButton } from "../CopyButton";
import { useSaves } from "../SavesContext";
import {
  getPatchesPath,
  getReplaysPath,
  getROMsPath,
  getSavesPath,
} from "../../../paths";
import path from "path";

const MATCH_TYPES = ["single", "triple"];

export default function PlayPane({ active }: { active: boolean }) {
  const { roms, rescan: rescanROMs } = useROMs();
  const { patches } = usePatches();
  const { saves } = useSaves();
  const { t, i18n } = useTranslation();

  const [selection_, setSelection] = React.useState<[string, string] | null>(
    null
  );
  const [incarnation, setIncarnation] = React.useState(0);

  const [matchType, setMatchType] = React.useState(0);
  const [linkCode, setLinkCode] = React.useState("");
  const [selectedSaveName, setSelectedSaveName] = React.useState<string | null>(
    null
  );
  const [started, setStarted] = React.useState(false);

  const selection =
    selection_ != null
      ? (() => {
          const [romName, patchName] = selection_;
          return Object.prototype.hasOwnProperty.call(roms, romName) &&
            (patchName == null ||
              Object.prototype.hasOwnProperty.call(patches, patchName))
            ? selection_
            : null;
        })()
      : null;

  const [romName, patchName] = selection ?? [null, null];
  const romInfo = romName != null ? KNOWN_ROMS[romName] : null;
  const patchInfo = patchName != null ? patches[patchName] : null;

  const romNames = Object.keys(roms);
  romNames.sort((k1, k2) => {
    const title1 = KNOWN_ROMS[k1].title[i18n.resolvedLanguage];
    const title2 = KNOWN_ROMS[k2].title[i18n.resolvedLanguage];
    return title1 < title2 ? -1 : title1 > title2 ? 1 : 0;
  });

  const patchVersions = React.useMemo(
    () =>
      patchInfo != null ? semver.rsort(Object.keys(patchInfo.versions)) : null,
    [patchInfo]
  );

  const [selectedPatchVersion, setSelectedPatchVersion] = React.useState<
    string | null
  >(null);
  React.useEffect(() => {
    setSelectedPatchVersion(patchVersions != null ? patchVersions[0] : null);
  }, [patchVersions]);

  const eligibleSaveNames = Object.keys(saves).filter(
    (selectedSaveName) => saves[selectedSaveName].romName == romName
  );
  eligibleSaveNames.sort();

  const netplayCompatibility =
    romInfo != null
      ? patchInfo != null &&
        selectedPatchVersion != null &&
        patchInfo.versions[selectedPatchVersion] != null
        ? patchInfo.versions[selectedPatchVersion].netplayCompatibility
        : romInfo.netplayCompatibility
      : "";

  const sessionID = `${netplayCompatibility}-${MATCH_TYPES[matchType]}-${linkCode}`;

  return (
    <Box
      sx={{
        minWidth: 0,
        flexGrow: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Stack sx={{ flexGrow: 1, my: 1 }} spacing={1}>
        <Box flexGrow={0} flexShrink={0} sx={{ px: 1 }}>
          <Stack spacing={1} direction="row">
            <FormControl fullWidth size="small">
              <InputLabel id="select-game-label">
                <Trans i18nKey="play:select-game" />
              </InputLabel>
              <Select
                labelId="select-game-label"
                value={selection != null ? JSON.stringify(selection) : ""}
                label={<Trans i18nKey="play:select-game" />}
                renderValue={(v) => {
                  if (v == "") {
                    return null;
                  }

                  const [romName, patchName] = JSON.parse(v);
                  return patchName != null ? (
                    <>
                      {patches[patchName].title}{" "}
                      <small>
                        {KNOWN_ROMS[romName].title[i18n.resolvedLanguage]}
                      </small>
                    </>
                  ) : (
                    KNOWN_ROMS[romName].title[i18n.resolvedLanguage]
                  );
                }}
                onChange={(e) => {
                  const [newROMName, newPatchName] = JSON.parse(
                    e.target.value as string
                  );
                  if (newROMName != romName) {
                    setSelectedSaveName(null);
                  }
                  setSelection([newROMName, newPatchName]);
                  setSelectedPatchVersion(null);
                }}
              >
                {romNames.map((romName) => {
                  const eligiblePatchNames = Object.keys(patches).filter(
                    (p) => patches[p].forROM == romName
                  );
                  eligiblePatchNames.sort();

                  return [
                    <ListSubheader key="title" sx={{ userSelect: "none" }}>
                      {KNOWN_ROMS[romName].title[i18n.resolvedLanguage]}
                    </ListSubheader>,
                    <MenuItem
                      key={romName}
                      value={JSON.stringify([romName, null])}
                    >
                      <Trans i18nKey="play:unpatched"></Trans>
                    </MenuItem>,
                    ...eligiblePatchNames.map((patchName) => {
                      const v = JSON.stringify([romName, patchName]);
                      return (
                        <MenuItem key={v} value={v}>
                          {patches[patchName].title}
                        </MenuItem>
                      );
                    }),
                  ];
                })}
              </Select>
            </FormControl>
            <Tooltip title={<Trans i18nKey="play:reload-roms" />}>
              <IconButton
                onClick={() => {
                  rescanROMs();
                }}
              >
                <RefreshIcon />
              </IconButton>
            </Tooltip>
          </Stack>
        </Box>
        {romInfo != null ? (
          <Box flexGrow={1} overflow="auto" sx={{ px: 1 }}>
            {patchInfo != null ? (
              <>
                <Typography variant="h6" sx={{ userSelect: "none" }}>
                  <Trans i18nKey="play:patch.authors" />
                </Typography>
                <Typography>
                  {patchInfo
                    .authors!.flatMap<React.ReactNode>((author, i) => {
                      const addr = parseOneAddress(
                        author
                      ) as ParsedMailbox | null;
                      if (addr == null) {
                        return [];
                      }
                      return (
                        <Tooltip title={addr.address}>
                          <Link href={`mailto:${addr.address}`} key={i}>
                            {addr.name}
                          </Link>
                        </Tooltip>
                      );
                    })
                    .reduce((prev, curr) => [prev, ", ", curr])}
                </Typography>
                <Typography variant="h6" sx={{ userSelect: "none" }}>
                  <Trans i18nKey="play:patch.version" />
                </Typography>
                <Select
                  size="small"
                  variant="standard"
                  value={selectedPatchVersion ?? ""}
                  onChange={(e) => {
                    setSelectedPatchVersion(e.target.value);
                  }}
                >
                  {patchVersions!.map((version) => {
                    return (
                      <MenuItem key={version} value={version}>
                        {version}
                      </MenuItem>
                    );
                  })}
                </Select>
              </>
            ) : null}
          </Box>
        ) : (
          <Box
            flexGrow={1}
            display="flex"
            justifyContent="center"
            alignItems="center"
            sx={{ userSelect: "none", color: "text.disabled" }}
          >
            <Stack alignItems="center" spacing={1}>
              <SportsEsportsOutlinedIcon sx={{ fontSize: "4rem" }} />
              <Typography variant="h6">
                <Trans i18nKey="play:no-game-selected" />
              </Typography>
            </Stack>
          </Box>
        )}

        <Stack
          flexGrow={0}
          flexShrink={0}
          direction="row"
          justifyContent="flex-end"
          spacing={1}
          sx={{ px: 1 }}
        >
          <Box flexGrow={1} flexShrink={0}>
            <TextField
              disabled={selection == null}
              size="small"
              label={<Trans i18nKey={"play:link-code"} />}
              value={linkCode}
              onChange={(e) => {
                setLinkCode(e.target.value.replace(/\s/g, "").toLowerCase());
              }}
              fullWidth
              InputProps={{
                startAdornment: (
                  <InputAdornment position="start" sx={{ mr: 0 }}>
                    {romInfo != null ? (
                      <>
                        {netplayCompatibility}-
                        <Select
                          variant="standard"
                          value={matchType}
                          onChange={(e) => {
                            setMatchType(e.target.value as number);
                          }}
                          renderValue={(v) => MATCH_TYPES[v]}
                          disabled={selection == null}
                        >
                          {MATCH_TYPES.map((v, k) => (
                            <MenuItem key={k} value={k}>
                              <ListItemText
                                primary={v}
                                secondary={
                                  k == 0
                                    ? t("play:match-type.single")
                                    : k == 1
                                    ? t("play:match-type.triple")
                                    : null
                                }
                              />
                            </MenuItem>
                          ))}
                        </Select>
                        -
                      </>
                    ) : null}
                  </InputAdornment>
                ),
                endAdornment: (
                  <InputAdornment position="end">
                    <CopyButton
                      disabled={selection == null}
                      value={sessionID}
                    />
                  </InputAdornment>
                ),
              }}
            />
          </Box>
          <Box flexGrow={1} flexShrink={0}>
            <FormControl fullWidth size="small">
              <InputLabel id="save-file-label">
                <Trans i18nKey="play:save-file"></Trans>
              </InputLabel>
              <Select
                labelId="save-file-label"
                disabled={selection == null}
                size="small"
                value={selectedSaveName || ""}
                label={<Trans i18nKey={"play:save-file"} />}
                onChange={(e) => {
                  setSelectedSaveName(e.target.value);
                }}
                fullWidth
              >
                {eligibleSaveNames.map((selectedSaveName) => {
                  return (
                    <MenuItem key={selectedSaveName} value={selectedSaveName}>
                      {selectedSaveName}
                    </MenuItem>
                  );
                })}
              </Select>
            </FormControl>
          </Box>
          <Button
            variant="contained"
            startIcon={<SportsMmaIcon />}
            disabled={
              selection == null || linkCode == "" || selectedSaveName == null
            }
            onClick={() => {
              setStarted(true);
            }}
          >
            <Trans i18nKey="play:fight" />
            {started ? (
              <CoreSupervisor
                incarnation={incarnation}
                romPath={path.join(
                  getROMsPath(),
                  roms[saves[selectedSaveName!].romName]
                )}
                patchPath={
                  selectedPatchVersion != null
                    ? path.join(
                        getPatchesPath(),
                        patchName!,
                        `v${selectedPatchVersion}.${
                          patchInfo!.versions[selectedPatchVersion].format
                        }`
                      )
                    : undefined
                }
                matchSettings={{
                  sessionID,
                  replaysPath: path.join(getReplaysPath()),
                  replayInfo: {
                    rom: romName!,
                    patch: { name: patchName!, version: selectedPatchVersion! },
                  },
                }}
                savePath={path.join(getSavesPath(), selectedSaveName!)}
                windowTitle={`${
                  KNOWN_ROMS[saves[selectedSaveName!].romName].title[
                    i18n.resolvedLanguage
                  ]
                }${
                  selectedPatchVersion != null ? ` + ${patchInfo!.title}` : ""
                }`}
                onExit={(_exitStatus) => {
                  setStarted(false);
                  setIncarnation((incarnation) => incarnation + 1);
                }}
              />
            ) : null}
          </Button>
        </Stack>
      </Stack>
    </Box>
  );
}
