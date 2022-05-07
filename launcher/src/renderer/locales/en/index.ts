import common from "./common.json";
import navbar from "./navbar.json";
import play from "./play.json";
import replays from "./replays.json";
import settings from "./settings.json";
import supervisor from "./supervisor.json";

export default {
  common,
  navbar,
  play,
  replays,
  settings,
  supervisor,
} as { [namespace: string]: { [key: string]: string } };
