import common from "./common.json";
import inputAxes from "./input-axes.json";
import inputButtons from "./input-buttons.json";
import inputKeys from "./input-keys.json";
import navbar from "./navbar.json";
import play from "./play.json";
import replays from "./replays.json";
import settings from "./settings.json";
import setup from "./setup.json";
import supervisor from "./supervisor.json";

export default {
  common,
  navbar,
  play,
  replays,
  settings,
  setup,
  supervisor,
  "input-keys": inputKeys,
  "input-buttons": inputButtons,
  "input-axes": inputAxes,
} as { [namespace: string]: { [key: string]: string } };
