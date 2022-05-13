import DiscordRPC from "discord-rpc";
import EventEmitter from "events";

const APP_ID = "974089681333534750";

let activity: DiscordRPC.Presence | undefined = undefined;
let ready = false;

const ACTIVITY_TEMPLATE = {
  smallImageKey: "logo",
  smallImageText: "Tango",
} as DiscordRPC.Presence;

export interface RichPresenceGameInfo {
  title: string;
  romName: string;
}

export function setLinkCode(
  linkCode: string,
  info: RichPresenceGameInfo | null
) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "Looking for match",
    details: info != null ? info.title : undefined,
    joinSecret: linkCode,
    partyId: `party:${linkCode}`,
    partySize: 1,
    partyMax: 2,
    largeImageKey: info != null ? info.romName.toLowerCase() : undefined,
    largeImageText: info != null ? info.title : undefined,
  };
  updateActivity();
}

export function setSinglePlayer(info: RichPresenceGameInfo | null) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "In single player",
    details: info != null ? info.title : undefined,
    largeImageKey: info != null ? info.romName.toLowerCase() : undefined,
    largeImageText: info != null ? info.title : undefined,
  };
  updateActivity();
}

export function setInLobby(
  linkCode: string,
  info: RichPresenceGameInfo | null
) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "In lobby",
    details: info != null ? info.title : undefined,
    partyId: `party:${linkCode}`,
    partySize: 2,
    partyMax: 2,
    largeImageKey: info != null ? info.romName.toLowerCase() : undefined,
    largeImageText: info != null ? info.title : undefined,
  };
  updateActivity();
}

export function setInProgress(
  linkCode: string,
  startTime: Date,
  info: RichPresenceGameInfo | null
) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "Match in progress",
    details: info != null ? info.title : undefined,
    partyId: `party:${linkCode}`,
    partySize: 2,
    partyMax: 2,
    startTimestamp: startTime.valueOf(),
    largeImageKey: info != null ? info.romName.toLowerCase() : undefined,
    largeImageText: info != null ? info.title : undefined,
  };
  updateActivity();
}

export function setDone() {
  activity = { ...ACTIVITY_TEMPLATE };
  updateActivity();
}

let rpc: DiscordRPC.Client | null = null;

function updateActivity() {
  if (rpc == null || !ready) {
    return;
  }
  rpc.setActivity(activity);
}

try {
  rpc = new DiscordRPC.Client({ transport: "ipc" });

  rpc.on("ready", () => {
    ready = true;
    setDone();
    setInterval(() => {
      updateActivity();
    }, 15 * 1000);

    // HACK: The types are actually incorrect, so we do this as a hack sadly.
    rpc!.subscribe("ACTIVITY_JOIN", undefined as any);
  });

  rpc.on("ACTIVITY_JOIN", (d: { secret: string }) => {
    events.emit("activityjoin", d);
  });

  rpc.login({ clientId: APP_ID });
} catch (e) {
  console.error("failed to initialize discord RPC", e);
}

export class Events extends EventEmitter {
  constructor() {
    super();
  }
}

export const events = new Events();

export declare interface Events {
  on(event: "activityjoin", f: (d: { secret: string }) => void): this;
  off(event: "activityjoin", f: (d: { secret: string }) => void): this;
}
