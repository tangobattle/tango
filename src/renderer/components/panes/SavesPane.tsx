import { Trans } from "react-i18next";
import React from "react";
import Stack from "@mui/material/Stack";
import Tabs from "@mui/material/Tabs";
import Tab from "@mui/material/Tab";
import Select from "@mui/material/Select";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";

export default function SavesPane({ active }: { active: boolean }) {
  const [tab, setTab] = React.useState("navicust");

  return (
    <Box
      sx={{
        p: 3,
        minWidth: 0,
        flexGrow: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Stack sx={{ flexGrow: 1 }} spacing={1}>
        <Box flexGrow={0} flexShrink={0}>
          <FormControl fullWidth size="small">
            <InputLabel id="select-save-label">
              <Trans i18nKey="saves:select-save" />
            </InputLabel>
            <Select
              labelId="select-save-label"
              label={<Trans i18nKey="saves:select-save" />}
            ></Select>
          </FormControl>
        </Box>
        {active ? (
          <Tabs
            value={tab}
            onChange={(e, value) => {
              setTab(value);
            }}
          >
            <Tab
              label={<Trans i18nKey="saves:tab.navicust" />}
              value="navicust"
            />
            <Tab label={<Trans i18nKey="saves:tab.folder" />} value="folder" />
            <Tab
              label={<Trans i18nKey="saves:tab.modcards" />}
              value="modcards"
              disabled
            />
          </Tabs>
        ) : null}
        <Box flexGrow={1} flexShrink={0}></Box>
        <Box textAlign="right">
          <Button variant="contained" startIcon={<PlayArrowIcon />}>
            <Trans i18nKey="saves:play" />
          </Button>
        </Box>
      </Stack>
    </Box>
  );
}
