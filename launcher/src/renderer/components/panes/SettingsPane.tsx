import { isEqual, sortBy } from "lodash-es";
import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { app, dialog, getCurrentWindow, shell } from "@electron/remote";
import AddIcon from "@mui/icons-material/Add";
import FolderOpenOutlinedIcon from "@mui/icons-material/FolderOpenOutlined";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import OpenInNewOutlinedIcon from "@mui/icons-material/OpenInNewOutlined";
import SportsEsportsIcon from "@mui/icons-material/SportsEsports";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import FormControl from "@mui/material/FormControl";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import InputLabel from "@mui/material/InputLabel";
import Link from "@mui/material/Link";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import { styled } from "@mui/material/styles";
import Tab from "@mui/material/Tab";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tabs from "@mui/material/Tabs";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { Config } from "../../../config";
import { captureInput } from "../../../input";
import { LANGUAGES } from "../../i18n";
import { useConfig } from "../ConfigContext";
import { usePatches } from "../PatchesContext";
import { useROMs } from "../ROMsContext";
import { useSaves } from "../SavesContext";

const AddChip = styled(Chip)(() => ({
  "&": {
    borderRadius: "12px",
  },
  "& .MuiChip-label": {
    padding: "0",
  },
}));
const KEYS = [
  "up",
  "down",
  "left",
  "right",
  "a",
  "b",
  "l",
  "r",
  "select",
  "start",
  "speedUp",
] as (keyof Config["inputMapping"])[];

function AboutTab({ active }: { active: boolean }) {
  return (
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0 }}
    >
      <Box
        sx={{
          display: "flex",
          py: 4,
          width: "100%",
          justifyContent: "center",
          userSelect: "none",
        }}
      >
        <Stack
          spacing={2}
          sx={{
            width: "500px",
          }}
        >
          <img
            src={require("../../../../static/images/logo.png")}
            width={160}
            height={160}
            alt="Tango"
            title="Tango"
            draggable={false}
            style={{ alignSelf: "center", display: "block" }}
          />
          <Typography variant="h4" sx={{ alignSelf: "center" }}>
            {app.getName()} <small>{app.getVersion()}</small>
          </Typography>
          <Typography>
            Tango would not be a reality without the work of the many people who
            have helped make this possible.
          </Typography>
          <Typography variant="h5" sx={{ alignSelf: "center" }}>
            Development
          </Typography>
          <ul>
            <li>
              <strong>Emulation:</strong>{" "}
              <Link
                href="https://twitter.com/endrift"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                endrift
              </Link>{" "}
              (
              <Link
                href="https://mgba.io/"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                mGBA
              </Link>
              )
            </li>
            <li>
              <strong>Reverse engineering:</strong>{" "}
              <Link
                href="https://twitter.com/pnw_ssbmars"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                pnw_ssbmars
              </Link>{" "}
              (BN3),{" "}
              <Link
                href="https://github.com/XKirby"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                XKirby
              </Link>{" "}
              (BN3),{" "}
              <Link
                href="https://github.com/luckytyphlosion"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                luckytyphlosion
              </Link>{" "}
              (BN6),{" "}
              <Link
                href="https://github.com/LanHikari22"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                LanHikari22
              </Link>{" "}
              (BN6),{" "}
              <Link
                href="https://www.nsa.gov"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                National Security Agency
              </Link>{" "}
              (
              <Link
                href="https://ghidra-sre.org//"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Ghidra
              </Link>
              ),{" "}
              <Link
                href="https://twitter.com/aldelaro5"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                aldelaro5
              </Link>{" "}
              (Ghidra),{" "}
              <Link
                href="https://twitter.com/GreigaMaster"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                GreigaMaster
              </Link>{" "}
              (BN),{" "}
              <Link
                href="https://twitter.com/Prof9"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Prof. 9
              </Link>{" "}
              (BN)
            </li>
            <li>
              <strong>Porting:</strong>{" "}
              <Link
                href="https://github.com/ubergeek77"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                ubergeek77
              </Link>{" "}
              (Linux),{" "}
              <Link
                href="https://twitter.com/pokemium"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Akatsuki
              </Link>{" "}
              (macOS)
            </li>
            <li>
              <strong>Game support:</strong>{" "}
              <Link
                href="https://github.com/bigfarts"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                weenie
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/GreigaMaster"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                GreigaMaster
              </Link>{" "}
              (EXE4.5)
            </li>
            <li>
              <strong>Odds and ends:</strong>{" "}
              <Link
                href="https://github.com/sailormoon"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                sailormoon
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/dev_console"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Shiz
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/Karate_Bugman"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Karate_Bugman
              </Link>
            </li>
          </ul>
          <Typography variant="h5" sx={{ alignSelf: "center" }}>
            Translation
          </Typography>
          <ul>
            <li>
              <strong>Japanese:</strong>{" "}
              <Link
                href="https://github.com/bigfarts"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                weenie
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/seventhfonist42"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Nonstopmop
              </Link>
            </li>
            <li>
              <strong>Simplified Chinese:</strong>{" "}
              <Link
                href="https://github.com/bigfarts"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                weenie
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/Hikari_Calyx"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Hikari Calyx
              </Link>
            </li>
            <li>
              <strong>Traditional Chinese:</strong>{" "}
              <Link
                href="https://github.com/bigfarts"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                weenie
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/Hikari_Calyx"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Hikari Calyx
              </Link>
            </li>
            <li>
              <strong>Spanish:</strong>{" "}
              <Link
                href="https://twitter.com/Karate_Bugman"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Karate_Bugman
              </Link>
            </li>
            <li>
              <strong>Brazilian Portuguese:</strong>{" "}
              <Link
                href="https://discord.gg/hPrFVaaRrU"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Darkgaia
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/mushiguchi"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                mushiguchi
              </Link>
            </li>
            <li>
              <strong>French:</strong>{" "}
              <Link
                href="https://twitter.com/Sheriel_Phoenix"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Sheriel Phoenix
              </Link>
              ,{" "}
              <Link
                href="https://twitter.com/justplayfly"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Justplay
              </Link>
            </li>
            <li>
              <strong>German:</strong>{" "}
              <Link
                href="https://www.twitch.tv/kendeep_fgc"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                KenDeep
              </Link>
            </li>
          </ul>
          <Typography variant="h5" sx={{ alignSelf: "center" }}>
            Art
          </Typography>
          <ul>
            <li>
              <strong>Logo:</strong>{" "}
              <Link
                href="https://twitter.com/saladdammit"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                saladdammit
              </Link>
            </li>
          </ul>
          <Typography variant="h5" sx={{ alignSelf: "center" }}>
            Special thanks
          </Typography>
          <ul>
            <li>
              <strong>Playtesting:</strong>{" "}
              <Link
                href="https://n1gp.net/"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                N1GP
              </Link>
            </li>
            <li>
              <strong>#1 fan:</strong>{" "}
              <Link
                href="https://twitter.com/Playerzero_exe"
                target="_blank"
                style={{ whiteSpace: "nowrap", hyphens: "none" }}
              >
                Playerzero_exe
              </Link>
            </li>
          </ul>
          <Typography>
            And, of course, a huge thank you to{" "}
            <Link
              href="https://www.capcom.com/"
              target="_blank"
              style={{ whiteSpace: "nowrap", hyphens: "none" }}
            >
              CAPCOM
            </Link>{" "}
            for making Mega Man Battle Network!
          </Typography>
          <Typography>Thank you everyone!</Typography>
          <Typography>
            Tango is licensed under the terms of the{" "}
            <Link
              href="https://tldrlegal.com/license/gnu-affero-general-public-license-v3-(agpl-3.0)"
              target="_blank"
            >
              GNU Affero General Public License v3
            </Link>
            . That means you’re free to modify the{" "}
            <Link href="https://github.com/tangobattle" target="_blank">
              source code
            </Link>{" "}
            of Tango, as long as you contribute your changes back!
          </Typography>
          <Typography variant="body2">
            Versions:{" "}
            {Object.entries(process.versions)
              .map(([k, v]) => `${k} ${v}`)
              .join(", ")}
          </Typography>
        </Stack>
      </Box>
    </Box>
  );
}

function GeneralTab({ active }: { active: boolean }) {
  const { config, save: saveConfig } = useConfig();
  const { i18n } = useTranslation();
  return (
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0 }}
    >
      <Box
        sx={{
          display: "flex",
          py: 4,
          width: "100%",
          justifyContent: "center",
        }}
      >
        <Stack
          spacing={2}
          sx={{
            width: "500px",
          }}
        >
          <TextField
            size="small"
            fullWidth
            value={config.nickname}
            onChange={(e) => {
              saveConfig((config) => ({
                ...config,
                nickname: e.target.value.substring(0, 32),
              }));
            }}
            label={<Trans i18nKey="settings:nickname" />}
          />
          <FormControl fullWidth size="small">
            <InputLabel id="language-label">
              <Trans i18nKey="settings:language" />
            </InputLabel>
            <Select
              labelId="language-label"
              value={i18n.resolvedLanguage}
              onChange={(e) => {
                i18n.changeLanguage(e.target.value);
              }}
              label={<Trans i18nKey="settings:language" />}
            >
              {LANGUAGES.map(({ code, name }) => (
                <MenuItem key={code} value={code}>
                  {name}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
          <TextField
            variant="outlined"
            size="small"
            type="number"
            label={<Trans i18nKey="settings:window-scale" />}
            value={config.windowScale}
            onChange={(e) => {
              let v = parseInt(e.target.value);
              if (isNaN(v)) {
                v = 0;
              }
              saveConfig((config) => ({
                ...config,
                windowScale: Math.min(Math.max(v, 1), 10),
              }));
            }}
            InputProps={{
              inputProps: {
                min: 1,
                max: 10,
              },
              endAdornment: (
                <InputAdornment position="end">
                  <Trans i18nKey="settings:window-scale-suffix" />
                </InputAdornment>
              ),
            }}
          />
          <FormControl fullWidth size="small">
            <InputLabel id="theme-label">
              <Trans i18nKey="settings:theme" />
            </InputLabel>
            <Select
              labelId="theme-label"
              value={config.theme}
              onChange={(e) => {
                saveConfig((config) => ({
                  ...config,
                  theme: e.target.value as "light" | "dark",
                }));
              }}
              label={<Trans i18nKey="settings:theme" />}
            >
              <MenuItem value="light">
                <Trans i18nKey="settings:theme.light" />
              </MenuItem>
              <MenuItem value="dark">
                <Trans i18nKey="settings:theme.dark" />
              </MenuItem>
            </Select>
          </FormControl>
        </Stack>
      </Box>
    </Box>
  );
}

function AdvancedTab({ active }: { active: boolean }) {
  const { config, save: saveConfig } = useConfig();
  return (
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0 }}
    >
      <Box
        sx={{
          display: "flex",
          py: 4,
          width: "100%",
          justifyContent: "center",
        }}
      >
        <Stack
          spacing={2}
          sx={{
            width: "500px",
          }}
        >
          <TextField
            size="small"
            fullWidth
            value={config.rustLogFilter}
            onChange={(e) => {
              saveConfig((config) => ({
                ...config,
                rustLogFilter: e.target.value,
              }));
            }}
            label={<Trans i18nKey="settings:rust-log-filter" />}
          />
          <TextField
            variant="outlined"
            size="small"
            type="number"
            label={<Trans i18nKey="settings:max-queue-length" />}
            value={config.maxQueueLength}
            onChange={(e) => {
              let v = parseInt(e.target.value);
              if (isNaN(v)) {
                v = 60;
              }
              saveConfig((config) => ({
                ...config,
                maxQueueLength: Math.min(Math.max(v, 0), 6000),
              }));
            }}
            InputProps={{
              inputProps: {
                min: 0,
                max: 6000,
              },
            }}
          />
          <TextField
            size="small"
            fullWidth
            value={config.endpoints.replaycollector}
            onChange={(e) => {
              saveConfig((config) => ({
                ...config,
                endpoints: {
                  ...config.endpoints,
                  replaycollector: e.target.value,
                },
              }));
            }}
            label={<Trans i18nKey="settings:replaycollector-endpoint" />}
          />
          <TextField
            size="small"
            fullWidth
            value={config.endpoints.signaling}
            onChange={(e) => {
              saveConfig((config) => ({
                ...config,
                endpoints: {
                  ...config.endpoints,
                  signaling: e.target.value,
                },
              }));
            }}
            label={<Trans i18nKey="settings:signaling-endpoint" />}
          />
          <TextField
            size="small"
            fullWidth
            value={config.endpoints.iceconfig}
            onChange={(e) => {
              saveConfig((config) => ({
                ...config,
                endpoints: {
                  ...config.endpoints,
                  iceconfig: e.target.value,
                },
              }));
            }}
            label={<Trans i18nKey="settings:iceconfig-endpoint" />}
          />
          <TextField
            size="small"
            fullWidth
            value={config.patchRepo}
            onChange={(e) => {
              saveConfig((config) => ({
                ...config,
                patchRepo: e.target.value,
              }));
            }}
            label={<Trans i18nKey="settings:patch-repo" />}
          />
          <FormControl fullWidth size="small">
            <InputLabel id="update-channel-label">
              <Trans i18nKey="settings:update-channel" />
            </InputLabel>

            <Select
              labelId="update-channel-label"
              value={config.updateChannel}
              onChange={(e) => {
                saveConfig((config) => ({
                  ...config,
                  updateChannel: e.target.value,
                }));
              }}
              label={<Trans i18nKey="settings:update-channel" />}
            >
              <MenuItem value="latest">
                <Trans i18nKey="settings:update-channel.latest" />
              </MenuItem>
              <MenuItem value="beta">
                <Trans i18nKey="settings:update-channel.beta" />
              </MenuItem>
              <MenuItem value="alpha">
                <Trans i18nKey="settings:update-channel.alpha" />
              </MenuItem>
              <MenuItem value="disabled">
                <Trans i18nKey="settings:update-channel.disabled" />
              </MenuItem>
            </Select>
          </FormControl>
          <Button
            fullWidth
            color="primary"
            variant="outlined"
            onClick={() => {
              shell.openPath(app.getPath("logs"));
            }}
          >
            <Trans i18nKey="settings:open-logs-folder" />
          </Button>
        </Stack>
      </Box>
    </Box>
  );
}

function DirectoryField(
  props: React.ComponentProps<typeof TextField> & {
    value: string;
    onPathChange: (path: string) => void;
  }
) {
  const { value, onPathChange, ...rest } = props;
  const onClick = React.useCallback(() => {
    const fn =
      dialog.showOpenDialogSync(getCurrentWindow(), {
        defaultPath: value,
        properties: ["openDirectory"],
      }) || [];
    if (fn.length == 0) {
      return;
    }
    onPathChange(fn[0]);
  }, [value, onPathChange]);

  return (
    <TextField
      onClick={onClick}
      value={value}
      {...rest}
      InputProps={{
        ...props.InputProps,
        readOnly: true,
        endAdornment: (
          <InputAdornment position="end">
            <Tooltip title={<Trans i18nKey="settings:browse" />}>
              <IconButton
                edge="end"
                onClick={(e) => {
                  e.stopPropagation();
                  onClick();
                }}
              >
                <FolderOpenOutlinedIcon />
              </IconButton>
            </Tooltip>
            <Tooltip title={<Trans i18nKey="settings:open-directory" />}>
              <IconButton
                edge="end"
                onClick={(e) => {
                  e.stopPropagation();
                  shell.openPath(value);
                }}
              >
                <OpenInNewOutlinedIcon />
              </IconButton>
            </Tooltip>
          </InputAdornment>
        ),
      }}
    />
  );
}

function PathsTab({ active }: { active: boolean }) {
  const { config, save: saveConfig } = useConfig();
  const { rescan: rescanPatches } = usePatches();
  const { rescan: rescanSaves } = useSaves();
  const { rescan: rescanROMs } = useROMs();

  React.useEffect(() => {
    rescanROMs();
  }, [config.paths.roms, rescanROMs]);

  React.useEffect(() => {
    rescanSaves();
  }, [config.paths.saves, rescanSaves]);

  React.useEffect(() => {
    rescanPatches();
  }, [config.paths.patches, rescanPatches]);

  return (
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0 }}
    >
      <Box
        sx={{
          display: "flex",
          py: 4,
          width: "100%",
          justifyContent: "center",
        }}
      >
        <Stack
          spacing={2}
          sx={{
            width: "500px",
          }}
        >
          <DirectoryField
            size="small"
            fullWidth
            onPathChange={(path) => {
              saveConfig((config) => ({
                ...config,
                paths: {
                  ...config.paths,
                  roms: path,
                },
              }));
            }}
            value={config.paths.roms}
            label={<Trans i18nKey="settings:paths.roms" />}
          />
          <DirectoryField
            size="small"
            fullWidth
            onPathChange={(path) => {
              saveConfig((config) => ({
                ...config,
                paths: {
                  ...config.paths,
                  saves: path,
                },
              }));
            }}
            value={config.paths.saves}
            label={<Trans i18nKey="settings:paths.saves" />}
          />
          <DirectoryField
            size="small"
            fullWidth
            onPathChange={(path) => {
              saveConfig((config) => ({
                ...config,
                paths: {
                  ...config.paths,
                  replays: path,
                },
              }));
            }}
            value={config.paths.replays}
            label={<Trans i18nKey="settings:paths.replays" />}
          />
          <DirectoryField
            size="small"
            fullWidth
            onPathChange={(path) => {
              saveConfig((config) => ({
                ...config,
                paths: {
                  ...config.paths,
                  patches: path,
                },
              }));
            }}
            value={config.paths.patches}
            label={<Trans i18nKey="settings:paths.patches" />}
          />
        </Stack>
      </Box>
    </Box>
  );
}

function InputTab({ active }: { active: boolean }) {
  const { config, save: saveConfig } = useConfig();
  const { i18n, t } = useTranslation();
  return (
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0 }}
    >
      <Box
        sx={{
          display: "flex",
          py: 4,
          width: "100%",
          justifyContent: "center",
        }}
      >
        <Stack spacing={1} sx={{ width: "500px" }}>
          <Table size="small">
            <TableBody>
              {KEYS.map((key) => (
                <TableRow key={key}>
                  <TableCell
                    component="th"
                    sx={{ width: "100px", verticalAlign: "top" }}
                  >
                    <strong>
                      <Trans i18nKey={`settings:input.${key}`} />
                    </strong>
                  </TableCell>
                  <TableCell>
                    <Box sx={{ mt: -1 }}>
                      {config.inputMapping[key].map((k, i) => (
                        <Chip
                          key={JSON.stringify(k)}
                          sx={{ mr: 1, mt: 1 }}
                          icon={
                            "Key" in k ? (
                              <KeyboardIcon />
                            ) : "Button" in k ? (
                              <SportsEsportsIcon />
                            ) : "Axis" in k ? (
                              <SportsEsportsIcon />
                            ) : undefined
                          }
                          label={
                            "Key" in k ? (
                              <Trans i18nKey={`input-keys:${k.Key}`}>
                                {k.Key}
                              </Trans>
                            ) : "Button" in k ? (
                              <Trans i18nKey={`input-buttons:${k.Button}`}>
                                {k.Button}
                              </Trans>
                            ) : "Axis" in k ? (
                              <Trans
                                i18nKey={`input-axes:${k.Axis[0]}${
                                  k.Axis[1] > 0 ? "+" : "-"
                                }`}
                              >
                                {k.Axis[0]}
                                {k.Axis[1] > 0 ? "+" : "-"}
                              </Trans>
                            ) : (
                              ""
                            )
                          }
                          onDelete={() => {
                            saveConfig((config) => ({
                              ...config,
                              inputMapping: {
                                ...config.inputMapping,
                                [key]: config.inputMapping[key].filter(
                                  (_, j) => i != j
                                ),
                              },
                            }));
                          }}
                        />
                      ))}
                      <AddChip
                        size="small"
                        sx={{ mr: 1, mt: 1 }}
                        variant="outlined"
                        label={<AddIcon />}
                        onClick={() => {
                          (async () => {
                            const input = await captureInput(
                              i18n.language,
                              t("settings:request-input", {
                                key: t(`settings:input.${key}`),
                              })
                            );
                            if (input == null) {
                              return;
                            }
                            saveConfig((config) => ({
                              ...config,
                              inputMapping: {
                                ...config.inputMapping,
                                [key]: sortBy(
                                  [
                                    ...config.inputMapping[key].filter(
                                      (v) => !isEqual(v, input)
                                    ),
                                    input,
                                  ],
                                  (v) =>
                                    "Key" in v
                                      ? [0, v.Key]
                                      : "Button" in v
                                      ? [1, v.Button]
                                      : "Axis" in v
                                      ? [2, ...v.Axis]
                                      : null
                                ),
                              },
                            }));
                          })();
                        }}
                      />
                    </Box>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Stack>
      </Box>
    </Box>
  );
}

export default function SettingsPane({ active }: { active: boolean }) {
  const [tab, setTab] = React.useState("general");

  return (
    <Box
      sx={{
        flexGrow: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Stack flexGrow={1} flexShrink={0} sx={{ width: 0 }}>
        <Tabs
          sx={{ px: 1 }}
          value={tab}
          onChange={(e, value) => {
            setTab(value);
          }}
        >
          <Tab
            label={<Trans i18nKey="settings:tab.general" />}
            value="general"
          />
          <Tab label={<Trans i18nKey="settings:tab.input" />} value="input" />
          <Tab label={<Trans i18nKey="settings:tab.paths" />} value="paths" />
          <Tab
            label={<Trans i18nKey="settings:tab.advanced" />}
            value="advanced"
          />
          <Tab label={<Trans i18nKey="settings:tab.about" />} value="about" />
        </Tabs>
        <GeneralTab active={tab == "general"} />
        <InputTab active={tab == "input"} />
        <PathsTab active={tab == "paths"} />
        <AdvancedTab active={tab == "advanced"} />
        <AboutTab active={tab == "about"} />
      </Stack>
    </Box>
  );
}
