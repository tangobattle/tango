import ThemeProvider from "@mui/system/ThemeProvider";
import React, { Suspense } from "react";
import theme from "../theme";
import { ConfigProvider } from "./ConfigContext";
import { ROMsProvider } from "./ROMsContext";
import { PatchesProvider } from "./PatchesContext";
import Box from "@mui/material/Box";
import CssBaseline from "@mui/material/CssBaseline";
import Navbar, { NavbarSelection } from "./Navbar";
import PlayPane from "./panes/PlayPane";
import SavesPane from "./panes/SavesPane";
import { scan } from "../../saves";

function AppBody() {
  const [selected, setSelected] = React.useState<NavbarSelection>("play");

  return (
    <Box display="flex" height="100%">
      <Navbar
        selected={selected}
        onSelect={(v) => {
          setSelected(v);
        }}
      />
      <PlayPane active={selected == "play"} />
      <SavesPane active={selected == "saves"} />
    </Box>
  );
}

export default function App() {
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <Suspense fallback={null}>
        <ConfigProvider>
          <ROMsProvider>
            <PatchesProvider>
              <AppBody />
            </PatchesProvider>
          </ROMsProvider>
        </ConfigProvider>
      </Suspense>
    </ThemeProvider>
  );
}
