import ThemeProvider from "@mui/system/ThemeProvider";
import { Trans } from "react-i18next";
import React, { Suspense } from "react";
import theme from "../theme";

import { ConfigProvider } from "./ConfigContext";
import { ROMsProvider } from "./ROMsContext";

export default function App(): JSX.Element {
  return (
    <ThemeProvider theme={theme}>
      <Suspense fallback={null}>
        <ConfigProvider>
          <ROMsProvider>
            <Trans i18nKey="hello">hello</Trans>
          </ROMsProvider>
        </ConfigProvider>
      </Suspense>
    </ThemeProvider>
  );
}
