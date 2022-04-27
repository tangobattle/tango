import * as log from "electron-log";
import React from "react";
import { render } from "react-dom";

import App from "./components/App";
import i18n from "./i18n";

Object.assign(console, log.functions);

const root = document.createElement("div");

root.id = "root";
document.body.appendChild(root);

i18n.on("languageChanged", (lng) => {
  document.documentElement.setAttribute("lang", lng);
});

render(<App />, document.getElementById("root"));
