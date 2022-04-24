import React from "react";
import { Trans, useTranslation } from "react-i18next";

import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Stack from "@mui/material/Stack";
import Tab from "@mui/material/Tab";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tabs from "@mui/material/Tabs";

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

function KeymappingTab({ active }: { active: boolean }) {
  const { config } = useConfig();
  const { t } = useTranslation();
  return (
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0, minWidth: 0 }}
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
                <TableCell>{config.keymapping[key]}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        <Button
          variant="contained"
          onClick={() => {
            (async () => {
              const keymaptool = new Keymaptool();
              for (const key of KEYS) {
                const mapped = await keymaptool.request(
                  t("settings:request-keymapping", {
                    key: t(`settings:keymapping.${key}`),
                  })
                );
                if (mapped == null) {
                  break;
                }
              }
              keymaptool.close();
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
      <Stack flexGrow={1} flexShrink={0}>
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
        </Tabs>
        <KeymappingTab active={tab == "keymapping"} />
      </Stack>
    </Box>
  );
}
