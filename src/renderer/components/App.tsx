import ThemeProvider from "@mui/system/ThemeProvider";
import { Trans, useTranslation } from "react-i18next";
import React, { Suspense } from "react";
import theme from "../theme";

import { ConfigProvider, useConfig } from "./ConfigContext";
import { ROMsProvider, useROMs } from "./ROMsContext";
import { PatchesProvider, usePatches } from "./PatchesContext";
import { KNOWN_ROMS } from "../../rom";
import { CoreSupervisor } from "./CoreSupervisor";

function PlayGameButton() {
  const { roms } = useROMs();
  const { patches } = usePatches();
  const { i18n } = useTranslation();

  const [selectedROM, setSelectedROM] = React.useState<string | null>(null);
  const [selectedPatch, setSelectedPatch] = React.useState<string | null>(null);
  const [started, setStarted] = React.useState(false);

  const romNames = Object.keys(roms);
  romNames.sort();

  const eligiblePatchNames = Object.keys(patches).filter(
    (p) => patches[p].forROM == selectedROM
  );
  eligiblePatchNames.sort();

  return (
    <>
      <div>
        <select
          size={4}
          style={{ width: "400px" }}
          onChange={(e) => {
            setSelectedPatch(null);
            setSelectedROM(e.target.value);
          }}
        >
          {romNames.map((romName) => {
            return (
              <option key={romName} value={romName}>
                {KNOWN_ROMS[romName].title[i18n.resolvedLanguage]}
              </option>
            );
          })}
        </select>
      </div>
      <div>
        <select
          size={4}
          style={{ width: "400px" }}
          onChange={(e) => {
            setSelectedPatch(e.target.value || null);
          }}
        >
          <option value={""}>(base game)</option>
          {eligiblePatchNames.map((patchName) => {
            return (
              <option key={patchName} value={patchName}>
                {patches[patchName].title}
              </option>
            );
          })}
        </select>
        <button
          onClick={() => {
            setStarted(true);
          }}
        >
          leg's go!
        </button>
        {started ? (
          <CoreSupervisor
            romName={selectedROM!}
            patchName={selectedPatch}
            sessionID="bingus"
            onExit={(exitStatus) => {
              setStarted(false);
            }}
          />
        ) : null}
      </div>
    </>
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
