import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { app, shell } from "@electron/remote";
import AddIcon from "@mui/icons-material/Add";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import SportsEsportsIcon from "@mui/icons-material/SportsEsports";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import FormControl from "@mui/material/FormControl";
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
import Typography from "@mui/material/Typography";

import { Config } from "../../../config";
import { captureInput } from "../../../input";
import { LANGUAGES } from "../../i18n";
import { useConfig } from "../ConfigContext";

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
          <ul>
            <li>
              <Link href="https://www.capcom.com/" target="_blank">
                CAPCOM
              </Link>{" "}
              for making Mega Man Battle Network!
            </li>
            <li>
              <Link href="https://twitter.com/endrift" target="_blank">
                endrift
              </Link>{" "}
              for their work on{" "}
              <Link href="https://mgba.io/" target="_blank">
                mGBA
              </Link>
              , which all of the emulation in Tango is based on.
            </li>
            <li>
              <Link href="https://twitter.com/pnw_ssbmars" target="_blank">
                pnw_ssbmars
              </Link>{" "}
              and{" "}
              <Link href="https://github.com/XKirby" target="_blank">
                XKirby
              </Link>{" "}
              for their work on{" "}
              <Link href="https://www.n1gp.dev/bbn3" target="_blank">
                BBN3
              </Link>
              , the original Battle Network rollback netplay, and indispensable
              assistance on Tango (and the name and logo!).
            </li>
            <li>
              <Link href="https://github.com/luckytyphlosion" target="_blank">
                luckytyphlosion
              </Link>{" "}
              and{" "}
              <Link href="https://github.com/LanHikari22" target="_blank">
                LanHikari22
              </Link>{" "}
              for their work on{" "}
              <Link href="https://github.com/dism-exe/bn6f" target="_blank">
                disassembling and documenting the Battle Network 6 code
              </Link>
              .
            </li>
            <li>
              <Link href="https://twitter.com/aldelaro5" target="_blank">
                aldelaro5
              </Link>{" "}
              and the{" "}
              <Link href="https://www.nsa.gov" target="_blank">
                National Security Agency
              </Link>{" "}
              for the help with and development of{" "}
              <Link href="https://ghidra-sre.org/" target="_blank">
                Ghidra
              </Link>
              .
            </li>
            <li>
              <Link href="https://twitter.com/GreigaMaster" target="_blank">
                GreigaMaster
              </Link>{" "}
              and{" "}
              <Link href="https://twitter.com/Prof9" target="_blank">
                Prof. 9
              </Link>{" "}
              for all of their original reverse engineering work on Battle
              Network 6.
            </li>
            <li>
              <Link href="https://twitter.com/saladdammit" target="_blank">
                saladdammit
              </Link>
              ,{" "}
              <Link href="https://twitter.com/Playerzero_exe" target="_blank">
                Playerzero_exe
              </Link>
              , and the entire{" "}
              <Link href="https://n1gp.net/" target="_blank">
                N1GP
              </Link>{" "}
              for their bug testing and support.
            </li>
            <li>
              <Link href="https://twitter.com/seventhfonist42" target="_blank">
                Nonstopmop
              </Link>{" "}
              for their contribution to the Japanese translation.
            </li>
            <li>
              <Link href="https://twitter.com/Hikari_Calyx" target="_blank">
                Hikari Calyx
              </Link>{" "}
              for their contribution to the Chinese translation.
            </li>
            <li>
              <Link href="https://github.com/bigfarts" target="_blank">
                bigfarts
              </Link>{" "}
              thats me lol
            </li>
          </ul>
          <Typography>Thank you!</Typography>
          <Typography>
            <small>
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
            </small>
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
              (async () => {
                saveConfig((config) => ({
                  ...config,
                  nickname: e.target.value.substring(0, 32),
                }));
              })();
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
          <FormControl fullWidth size="small">
            <InputLabel id="theme-label">
              <Trans i18nKey="settings:theme" />
            </InputLabel>
            <Select
              labelId="theme-label"
              value={config.theme}
              onChange={(e) => {
                (async () => {
                  saveConfig((config) => ({
                    ...config,
                    theme: e.target.value as "light" | "dark",
                  }));
                })();
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
              (async () => {
                saveConfig((config) => ({
                  ...config,
                  rustLogFilter: e.target.value,
                }));
              })();
            }}
            label={<Trans i18nKey="settings:rust-log-filter" />}
          />
          <TextField
            size="small"
            fullWidth
            value={config.matchmakingServerAddr}
            onChange={(e) => {
              (async () => {
                saveConfig((config) => ({
                  ...config,
                  matchmakingServerAddr: e.target.value,
                }));
              })();
            }}
            label={<Trans i18nKey="settings:matchmaking-server-addr" />}
          />
          <FormControl fullWidth size="small">
            <InputLabel id="update-channel-label">
              <Trans i18nKey="settings:update-channel" />
            </InputLabel>

            <Select
              labelId="update-channel-label"
              value={config.updateChannel}
              onChange={(e) => {
                (async () => {
                  saveConfig((config) => ({
                    ...config,
                    updateChannel: e.target.value,
                  }));
                })();
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
                          key={i}
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
                                [key]: [...config.inputMapping[key], input],
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
          <Tab
            label={<Trans i18nKey="settings:tab.controls" />}
            value="input"
          />
          <Tab
            label={<Trans i18nKey="settings:tab.advanced" />}
            value="advanced"
          />
          <Tab label={<Trans i18nKey="settings:tab.about" />} value="about" />
        </Tabs>
        <GeneralTab active={tab == "general"} />
        <InputTab active={tab == "input"} />
        <AdvancedTab active={tab == "advanced"} />
        <AboutTab active={tab == "about"} />
      </Stack>
    </Box>
  );
}
