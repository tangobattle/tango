import "./i18n";

import * as log from "electron-log";
import React from "react";
import { render } from "react-dom";

import App from "./components/App";

Object.assign(console, log.functions);

const root = document.createElement("div");

root.id = "root";
document.body.appendChild(root);

render(<App />, document.getElementById("root"));
