import ThemeProvider from "@mui/system/ThemeProvider";
import { Trans, useTranslation } from "react-i18next";
import React, { Suspense } from "react";
import theme from "../theme";
import { ConfigProvider, useConfig } from "./ConfigContext";
import { ROMsProvider, useROMs } from "./ROMsContext";
import { PatchesProvider, usePatches } from "./PatchesContext";
import { KNOWN_ROMS } from "../../rom";
import { CoreSupervisor } from "./CoreSupervisor";
import { findPatchVersion } from "../../patchinfo";
import Select from "@mui/material/Select";
import ListSubheader from "@mui/material/ListSubheader";
import MenuItem from "@mui/material/MenuItem";
import Box from "@mui/material/Box";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";

function PlayGameButton() {
  const { roms } = useROMs();
  const { patches } = usePatches();
  const { i18n } = useTranslation();

  const [selection, setSelection] = React.useState<string | null>();
  const [started, setStarted] = React.useState(false);

  const romNames = Object.keys(roms);
  romNames.sort();

  return (
    <Box>
      <FormControl sx={{ width: "100%" }} size="small">
        <InputLabel>
          <Trans i18nKey="select-game"></Trans>
        </InputLabel>
        <Select
          variant="standard"
          value={selection}
          renderValue={(v) => {
            if (v == null) {
              return null;
            }
            const [romName, ...rest] = v.split("+");
            const patchName = rest.join("+");
            let name = KNOWN_ROMS[romName].title[i18n.resolvedLanguage];
            if (patchName != "") {
              name += ` + ${patches[patchName].title}`;
            }
            return name;
          }}
          onChange={(event) => {
            setSelection(event.target.value);
          }}
        >
          {romNames.map((romName) => {
            const eligiblePatchNames = Object.keys(patches).filter(
              (p) => patches[p].forROM == romName
            );
            eligiblePatchNames.sort();

            return [
              <ListSubheader key="title">
                {KNOWN_ROMS[romName].title[i18n.resolvedLanguage]}
              </ListSubheader>,
              <MenuItem key={romName} value={romName}>
                Unpatched
              </MenuItem>,
              ...eligiblePatchNames.map((patchName) => {
                return (
                  <MenuItem
                    key={romName + "+" + patchName}
                    value={romName + "+" + patchName}
                  >
                    {patches[patchName].title}
                  </MenuItem>
                );
              }),
            ];
          })}
        </Select>
      </FormControl>
    </Box>
  );
}

export default function App() {
  return (
    <ThemeProvider theme={theme}>
      <Suspense fallback={null}>
        <ConfigProvider>
          <ROMsProvider>
            <PatchesProvider>
              <PlayGameButton />
            </PatchesProvider>
          </ROMsProvider>
        </ConfigProvider>
      </Suspense>
    </ThemeProvider>
  );
}
