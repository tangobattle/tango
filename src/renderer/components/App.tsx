import ThemeProvider from "@mui/system/ThemeProvider";
import React, { Suspense } from "react";
import theme from "../theme";

import { ConfigProvider } from "./ConfigContext";

export default function App(): JSX.Element {
  return (
    <ThemeProvider theme={theme}>
      <Suspense fallback={null}>
        <ConfigProvider></ConfigProvider>
      </Suspense>
    </ThemeProvider>
  );
}
