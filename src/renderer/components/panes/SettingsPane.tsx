import React from "react";
import { Trans } from "react-i18next";

import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Stack from "@mui/material/Stack";
import Tab from "@mui/material/Tab";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tabs from "@mui/material/Tabs";

import { useConfig } from "../ConfigContext";

function KeymappingTab({ active }: { active: boolean }) {
  const { config } = useConfig();
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
            {(
              [
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
              ] as (keyof typeof config.keymapping)[]
            ).map((key) => (
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
        <Button variant="contained">
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
