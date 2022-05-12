import DiscordRPC from "discord-rpc";

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
    details: gameTitle ?? undefined,
    state: "Looking for match",
    joinSecret: code,
    partyId: `party:${code}`,
  };
  updateActivity();
}

export function setInLobby(gameTitle: string | null) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    state: "In lobby",
    details: gameTitle ?? undefined,
  };
  updateActivity();
}

export function setInProgress(startTime: Date, gameTitle: string) {
  activity = {
    ...ACTIVITY_TEMPLATE,
    details: gameTitle,
    state: "Match in progress",
    startTimestamp: startTime.valueOf(),
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
  }, 15e3);

  rpc.subscribe("ACTIVITY_JOIN", (d) => {
    console.log(d);
  });
});

rpc.login({ clientId: APP_ID });
