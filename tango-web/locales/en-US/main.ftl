## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = English
window-quit = Exit Tango
tab-play = Play
tab-replays = Replays
tab-patches = Patches
tab-settings = Settings
play-no-game = No game selected
play-no-save = Select save
play-no-patch = No patch
play-version-placeholder = —
play-link-code = Link code (leave empty for a random one)
play-play = Play
play-fight = Fight
play-cancel = Leave
play-no-selection = Select a game and a save to inspect.
play-status-connecting = Connecting to matchmaking server…
play-status-waiting-opponent = Waiting for opponent…
play-you = You
play-opponent = Opponent
empty-no-roms-title = No game ROMs found
empty-no-roms-body = Drop your Battle Network / Rockman EXE .gba files into:
lobby-waiting = Waiting…
lobby-latency = Ping: { $ms } ms
lobby-link-code = Link code: { $code }
lobby-match-type = Match type
lobby-ready = Ready
lobby-unready = Unready
lobby-match-starting = Starting…
lobby-compat-ok = Compatible — ready to play.
lobby-compat-missing-game = One side hasn't picked a game.
lobby-compat-missing-rom = Game or patch isn't installed on both sides.
lobby-compat-match-mismatch = Match type doesn't match.
lobby-pick-game-first = Pick a game first
lobby-no-match-types = (no match types for this game)
session-results-victory = Victory!
session-results-defeat = Defeat
session-results-draw = Draw
session-results-no-contest = Match ended
session-results-vs = vs { $nickname }
session-results-you = You
session-results-round = Round { $number }
session-results-draws = { $count ->
    [one] 1 round ended in a draw
   *[other] { $count } rounds ended in a draw
}
session-results-done = Done
discord-presence-in-single-player = In single player
discord-presence-in-progress = Match in progress
playback-pause = Pause
playback-close = Close
replays-watch = Watch
replays-incomplete = incomplete
replays-watch-missing-rom = Watch (ROM for this game isn't scanned)
save-delete = Delete
patches-update = Update
patches-updating = Updating…
patches-update-failed = Update failed: { $error }
patches-netplay-compatibility = Netplay compatibility:
patches-details-games = Supported games:
patches-select-prompt = Select a patch.
settings-patch-repo = Patches repository
settings-section-general = General
settings-section-graphics = Graphics
settings-section-audio = Audio
settings-section-input = Input
settings-section-about = About
settings-volume = Volume
settings-nickname = Nickname
settings-language = Language
settings-input-press-key = Press a key or button…
settings-input-reset = Reset to defaults
settings-input-select-hint = Click a button to edit its bindings
welcome-title = Welcome to Tango!
welcome-subtitle = There's just a few steps you'll need to complete before you can start playing.
welcome-continue = Continue
welcome-step-roms = Add your ROMs
welcome-step-roms-detected = { $count } ROMs detected.
welcome-step-nickname = Set your nickname
welcome-roms-needed = Add at least one ROM before continuing.

## Web-client-only strings (concepts the desktop doesn't have).
## Untranslated locales fall back to en-US, the desktop's own policy
## for client-specific keys.
web-import = Import…
web-export = Export save
web-import-privacy = Files are copied into private browser storage and never leave this device.
web-replays-empty = No replays yet — finish a netplay match and it lands here. Downloaded replays open in the desktop client too.
web-download = Download
web-booting-replay = Booting replay…
web-menu = Menu
web-reset = Reset
web-diagnostics = Diagnostics
web-diagnostics-description = Runs a fixed-input lockstep match on this machine and hashes every settled state checkpoint. Comparing the stream hash against the native probe (same ROM + save) proves the browser simulates bit-identically to the desktop — the crossplay prerequisite.
web-diagnostics-run = Run
web-diagnostics-running = Running…
web-diagnostics-pick = pick a game and a real save on the Play tab first

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = NaviCust
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-empty = This save has no data for this view.
save-copy = Copy
save-copy-image = Copy as image
copied = Copied!
save-edit = Edit
save-edit-save = Save
save-edit-cancel = Cancel
save-edit-sort = Sort
save-edit-clear = Clear all
folder-group = Group by chip
navi-style = Style
navi-style-unset = (no style)
navi-id = Navi ID
navi-link-navi = Link Navi
navi-base-hp = HP
navi-buster = Buster
navi-buster-attack = Attack
navi-buster-rapid = Rapid
navi-buster-charge = Charge
navi-power-attack = Power Attack
navi-edit-select = Navi
folder-edit-search = Search chips…
folder-edit-folder = Folder
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
folder-sort-id = ID
folder-sort-name = ABCDE
folder-sort-code = Code
folder-sort-attack = Attack
folder-sort-element = Element
folder-sort-mb = MB
navicust-grid-size = Grid: { $cols } × { $rows }
navicust-parts = Installed parts
navicust-empty = (none installed)
navicust-edit-grid = NaviCust
navicust-edit-count = { $count ->
    [one] 1 part
   *[other] { $count } parts
}
navicust-edit-rotate = Rotate
navicust-edit-compress = Compress
navicust-edit-uncompress = Uncompress
navicust-edit-search = Search parts…
navicust-sort-id = ID
navicust-sort-name = ABCDE
navicust-sort-color = Color
patch-card-edit-search = Search cards…
patch-card-edit-count = { $count ->
    [one] 1 card
   *[other] { $count } cards
}
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = ABCDE
patch-card-sort-mb = MB
patch-card4-none = None
auto-battle-data-secondary-standard-chips = Standard chips (secondary)
auto-battle-data-standard-chips = Standard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combos
auto-battle-data-program-advance = Program advance
auto-battle-data-edit-used = Used
auto-battle-data-edit-secondary = Sec.
auto-battle-data-edit-count = { $count ->
    [one] 1 chip
   *[other] { $count } chips
}
save-actions = Save actions
save-duplicate = Duplicate
save-rename = Rename
save-rename-confirm = Rename
save-delete-confirm = Delete
save-action-cancel = Cancel
save-delete-prompt = Delete { $name }?
save-name-placeholder = New name
save-new = New save
save-new-confirm = Create
save-template-default = (default)
save-template-pick = Pick a template…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = All games
replays-filter-any-time = Any time
replays-filter-past-day = Past 24 hours
replays-filter-past-week = Past week
replays-filter-past-month = Past month
replays-filter-past-year = Past year
replays-filter-search-placeholder = Search replays…
replays-show-incomplete = Show incomplete
replays-direct-marker = (direct)
replays-select-prompt = Select a replay.
replays-match-type = Match type:
replays-duration = Duration:
replays-round-count = { $count ->
    [one] 1 round
   *[other] { $count } rounds
}

## patches detail (extracted from the desktop's main.ftl; keep in sync)
patches-favorite = Favorite
patches-unfavorite = Unfavorite
patches-search-placeholder = Search patches…
patches-readme-placeholder = This patch has no README.
patches-details-authors = Authors:
patches-details-license = License:
patches-details-source = Source:

## netplay settings (extracted from the desktop's main.ftl; keep in sync)
settings-matchmaking-endpoint = Matchmaking endpoint
settings-use-relay = Use relay server
settings-use-relay-auto = Auto
settings-use-relay-always = Always
settings-use-relay-never = Never
settings-show-opponent-setup = Show opponent's setup at match start

## netplay settings section label (extracted from the desktop)
settings-section-netplay = Netplay

## accent + patch repo settings (extracted from the desktop)
settings-accent = Accent color
settings-accent-tango-green = Tango Green
settings-accent-megaman-blue = MegaMan Blue
settings-accent-protoman-red = ProtoMan Red
settings-accent-roll-pink = Roll Pink
settings-accent-gutsman-yellow = GutsMan Yellow
settings-accent-bass-purple = Bass Purple

## welcome step description (extracted from the desktop)
welcome-step-nickname-description = You can change this at any time in Settings.

## replay video export (extracted from the desktop)
replays-export = Render
replays-export-progress = Rendering…
replays-export-cancel = Cancel
replays-export-success = Render finished.
replays-export-error = Render failed: { $error }

## theme + streamer + autoupdate settings (extracted from the desktop)
settings-theme = Theme
settings-theme-dark = Dark
settings-theme-light = Light
settings-streamer-mode = Streamer privacy mode
settings-enable-patch-autoupdate = Automatically update patches in the background

## mute bgm setting (extracted from the desktop)
settings-disable-bgm-in-pvp = Mute music in netplay

## cover tab (extracted from the desktop)
save-tab-cover = Cover

## lobby + telemetry + transport (extracted from the desktop)
lobby-blind-mine = Blind setup
lobby-blind-self-on = You are hiding your setup.
lobby-blind-peer-on = Opponent is hiding their setup.
settings-netplay-frame-delay = Frame delay
lobby-frame-delay-suggest = Suggest based on ping
session-stat-tps = Tick/s (current/max)
session-stat-skew = Skew
session-stat-lead = Lead
session-stat-depth = Misprediction depth
session-stat-ping = Network latency
playback-speed = Speed
playback-input-display = Input display

## replay input display + swap (extracted from the desktop)
playback-swap-perspective = Opponent's perspective

## pvp setup drawers (extracted from the desktop)
session-self = My setup
session-opponent = Opponent setup

## replay pip (extracted from the desktop's main.ftl; keep in sync)
playback-pip = Opponent screen
