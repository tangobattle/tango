import common from "./common.json";
import supervisor from "./supervisor.json";
import play from "./play.json";
import navbar from "./navbar.json";
import saves from "./saves.json";

export default {
  common,
  supervisor,
  play,
  navbar,
  saves,
} as { [namespace: string]: { [key: string]: string } };
