import ThemeProvider from "@mui/system/ThemeProvider";
import React, { Suspense } from "react";
import theme from "../theme";

import { ConfigProvider } from "./ConfigContext";
import { ROMsProvider } from "./ROMsContext";

export default function App(): JSX.Element {
  return (
    <ThemeProvider theme={theme}>
      <Suspense fallback={null}>
        <ConfigProvider>
          <ROMsProvider></ROMsProvider>
        </ConfigProvider>
      </Suspense>
    </ThemeProvider>
  );
}
