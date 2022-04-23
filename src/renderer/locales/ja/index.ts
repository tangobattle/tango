import common from "./common.json";
import supervisor from "./supervisor.json";
import play from "./play.json";
import navbar from "./navbar.json";

export default {
  common,
  supervisor,
  play,
  navbar,
} as { [namespace: string]: { [key: string]: string } };
