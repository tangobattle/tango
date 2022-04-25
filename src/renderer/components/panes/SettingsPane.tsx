import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { app } from "@electron/remote";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Link from "@mui/material/Link";
import Stack from "@mui/material/Stack";
import Tab from "@mui/material/Tab";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tabs from "@mui/material/Tabs";
import Typography from "@mui/material/Typography";

import { Config } from "../../../config";
import { Keymaptool } from "../../../input";
import { useConfig } from "../ConfigContext";

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
] as (keyof Config["keymapping"])[];

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
            width: "600px",
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
            Tango would not be possible without the work of the many people who
            have helped make this possible.
          </Typography>
          <ul>
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
              assistance on Tango (and the name!).
            </li>
            <li>
              <Link href="https://github.com/luckytyphlosion" target="_blank">
                luckytyphlosion
              </Link>{" "}
              for their work on{" "}
              <Link href="https://github.com/dism-exe/bn6f" target="_blank">
                disassembling and documenting the Battle Network 6 code
              </Link>
              .
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
              for their bug testing and support!
            </li>
            <li>
              <Link href="https://github.com/bigfarts" target="_blank">
                bigfarts
              </Link>{" "}
              thats me lol
            </li>
          </ul>
          <Typography>Thank you!</Typography>
        </Stack>
      </Box>
    </Box>
  );
}

function KeymappingTab({ active }: { active: boolean }) {
  const { config, save: saveConfig } = useConfig();
  const { t } = useTranslation();
  const keymaptoolRef = React.useRef<Keymaptool | null>(null);
  return (
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0 }}
    >
      <Stack spacing={1}>
        <Table size="small">
          <TableBody>
            {KEYS.map((key) => (
              <TableRow key={key}>
                <TableCell component="th">
                  <strong>
                    <Trans i18nKey={`settings:keymapping.${key}`} />
                  </strong>
                </TableCell>
                <TableCell sx={{ textAlign: "right" }}>
                  {config.keymapping[key]}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        <Button
          variant="contained"
          onClick={() => {
            (async () => {
              if (keymaptoolRef.current != null) {
                return;
              }
              keymaptoolRef.current = new Keymaptool();
              for (const key of KEYS) {
                const mapped = await keymaptoolRef.current.request(
                  t("settings:request-keymapping", {
                    key: t(`settings:keymapping.${key}`),
                  })
                );
                if (mapped == null) {
                  break;
                }
                await saveConfig((config) => ({
                  ...config,
                  keymapping: { ...config.keymapping, [key]: mapped },
                }));
              }
              keymaptoolRef.current.close();
              keymaptoolRef.current = null;
            })();
          }}
        >
          <Trans i18nKey="settings:remap" />
        </Button>
      </Stack>
    </Box>
  );
}

export default function SettingsPane({ active }: { active: boolean }) {
  const [tab, setTab] = React.useState("keymapping");

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
            label={<Trans i18nKey="settings:tab.keymapping" />}
            value="keymapping"
          />
          <Tab label={<Trans i18nKey="settings:tab.about" />} value="about" />
        </Tabs>
        <KeymappingTab active={tab == "keymapping"} />
        <AboutTab active={tab == "about"} />
      </Stack>
    </Box>
  );
}
