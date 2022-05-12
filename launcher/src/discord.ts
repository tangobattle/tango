import DiscordRPC from "discord-rpc";
import EventEmitter from "events";

const APP_ID = "974089681333534750";

const rpc = new DiscordRPC.Client({ transport: "ipc" });

let activity: DiscordRPC.Presence | undefined = undefined;
let ready = false;

const ACTIVITY_TEMPLATE = {
  smallImageKey: "logo",
  smallImageText: "Tango",
} as DiscordRPC.Presence;

export function setLinkCode(code: string, gameTitle: string | null) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "Looking for match",
    details: gameTitle ?? undefined,
    joinSecret: code,
    partyId: `party:${code}`,
    partySize: 1,
    partyMax: 2,
    largeImageKey: undefined, // TODO
    largeImageText: gameTitle ?? undefined,
  };
  updateActivity();
}

export function setInLobby(gameTitle: string | null) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "In lobby",
    details: gameTitle ?? undefined,
    partySize: 2,
    partyMax: 2,
    largeImageKey: undefined, // TODO
    largeImageText: gameTitle ?? undefined,
  };
  updateActivity();
}

export function setInProgress(startTime: Date, gameTitle: string) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "Match in progress",
    details: gameTitle,
    partySize: 2,
    partyMax: 2,
    startTimestamp: startTime.valueOf(),
    largeImageKey: undefined, // TODO
    largeImageText: gameTitle ?? undefined,
  };
  updateActivity();
}

export function setDone() {
  activity = { ...ACTIVITY_TEMPLATE };
  updateActivity();
}

function updateActivity() {
  if (!ready) {
    return;
  }
  rpc.setActivity(activity);
}

rpc.on("ready", () => {
  ready = true;
  setDone();
  setInterval(() => {
    updateActivity();
  }, 15 * 1000);

  // HACK: The types are actually incorrect, so we do this as a hack sadly.
  rpc.subscribe("ACTIVITY_JOIN", undefined as any);
});

rpc.on("ACTIVITY_JOIN", (d: { secret: string }) => {
  events.emit("activityjoin", d);
});

rpc.login({ clientId: APP_ID });

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
