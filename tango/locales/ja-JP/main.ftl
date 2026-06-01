# Window
# Endonym for this locale; shown in the language picker.
LANGUAGE = English
window-title = Tango
# Crash handler dialogs (parent process)
crash =
    おっと、Tangoがエラーに遭遇し、クラッシュしてしまいました。
    
    このクラッシュを報告する際には、以下のログファイルを添付してください。
    
    { $path }
crash-no-log =
    おっと、Tangoがエラーに遭遇し、クラッシュしてしまいました。
    
    { $error }
# Discord rich presence
discord-presence-looking = Looking for match
discord-presence-in-single-player = In single player
discord-presence-in-lobby = In lobby
discord-presence-in-progress = Match in progress
# Top-bar tabs
tab-play = Play
tab-replays = Replays
tab-patches = Patches
tab-settings = Settings
# Play selectors
play-no-game = No game selected
play-no-save = Select save
# Save management
save-open-folder = Open folder
save-duplicate = Duplicate
save-rename = Rename
save-delete = Delete
save-rename-confirm = Save
save-delete-confirm = Delete
save-action-cancel = Cancel
save-delete-prompt = Delete this save?
save-name-placeholder = New name
save-new = New save
save-new-confirm = Create
save-template-default = (default)
save-template-pick = Pick a template…
# Empty-state hints
empty-no-roms-title = No game ROMs found
empty-no-roms-body = Drop your Battle Network / Rockman EXE .gba files into:
empty-no-saves-title = No save files for this game
empty-no-saves-body = Drop a .sav for this game into:
play-no-patch = No patch
play-version-placeholder = —
# Play bottom strip
play-link-code = Link code
play-link-code-random = Random link code
play-play = Play
play-fight = Fight
play-cancel = Leave
play-status-idle = Enter a link code to start netplay, or leave blank for single-player.
play-status-connecting = Connecting to matchmaking server…
play-status-direct-connecting = Connecting to opponent…
play-status-waiting-opponent = Waiting for opponent…
play-status-negotiating = Negotiating…
play-status-failed = Connection failed: { $error }
play-status-peer-disconnected = The other player left.
play-status-negotiate-expected-hello = The other player didn't send the expected handshake.
play-status-negotiate-version-too-old = The other player is running an older version of Tango.
play-status-negotiate-version-too-new = The other player is running a newer version of Tango.
play-status-negotiate-failed = An error occurred during negotiation: { $error }
lobby-waiting = Waiting…
lobby-no-game = (no game selected)
lobby-latency = Ping: { $ms } ms
lobby-link-code = Link code: { $code }
lobby-direct-host = Hosting on port: { $port }
lobby-direct-connect = Connecting to: { $target }
lobby-handshake = Exchanging settings…
lobby-match-type = Match type
lobby-frame-delay-suggest = Suggest based on ping
lobby-no-match-types = (no match types for this game)
lobby-pick-game-first = Pick a game first
lobby-compat-ok = Compatible — ready to play.
lobby-compat-missing-game = One side hasn't picked a game.
lobby-compat-missing-rom = Game or patch isn't installed on both sides.
lobby-compat-version-mismatch = Game versions don't match (different patch / ROM).
lobby-compat-match-mismatch = Match type doesn't match.
lobby-ready = Ready
lobby-unready = Unready
lobby-match-starting = Starting…
lobby-reveal-mine = Reveal my setup to opponent
lobby-reveal-peer-on = Opponent is revealing their setup.
lobby-reveal-peer-off = Opponent isn't revealing.
lobby-reveal-peer-unknown = (waiting on opponent)
session-opponent = Opponent setup
session-self = My setup
session-back-to-session = Back to session
# Save view sub-tabs
save-tab-cover = Cover
save-tab-navi = Navi
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
# Navi pane
navi-style = Style
# Folder pane
folder-group = Group by chip
save-copy = Copy
save-copy-image = Copy as image
save-edit = Edit
save-edit-save = Save
save-edit-cancel = Cancel
folder-edit-search = Search chips…
folder-edit-folder = Folder
folder-edit-count = { $count } / 30
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
save-edit-sort = Sort
save-edit-clear = Clear all
folder-sort-id = ID
folder-sort-name = ABCDE
folder-sort-code = Code
folder-sort-attack = Attack
folder-sort-element = Element
folder-sort-mb = MB
# Navi pane
navi-id = Navi ID
navi-style-unset = (no style)
navicust-grid-size = Grid: { $cols } × { $rows }
navicust-parts = Installed parts
navicust-empty = (none installed)
# Navicust editor
navicust-edit-grid = NaviCust
navicust-edit-count =
    { $count ->
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
# Patch card editor
patch-card-edit-search = Search cards…
patch-card-edit-count =
    { $count ->
        [one] 1 card
       *[other] { $count } cards
    }
patch-card-edit-mb = { $mb }MB / 80MB
patch-card-sort-id = ID
patch-card-sort-name = ABCDE
patch-card-sort-mb = MB
patch-card4-none = None
# Auto Battle Data pane
auto-battle-data-secondary-standard-chips = Standard chips (secondary)
auto-battle-data-standard-chips = Standard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combos
auto-battle-data-program-advance = Program advance
# Auto Battle Data editor
auto-battle-data-edit-used = Used
auto-battle-data-edit-secondary = Sec.
auto-battle-data-edit-count =
    { $count ->
        [one] 1 chip
       *[other] { $count } chips
    }
# Common
save-empty = This save has no data for this view.
play-no-selection = Select a game and a save to inspect.
# Replays
replays-filter-all-games = All games
replays-filter-opponent-placeholder = Search opponents…
replays-show-incomplete = Show incomplete
replays-direct-marker = (direct)
replays-watch = Watch
replays-watch-missing-rom = Watch (ROM for this game isn't scanned)
replays-export = Render
replays-export-progress = Rendering…
replays-export-cancel = Cancel
replays-export-cancelling = Cancelling…
replays-export-success = Render finished.
replays-export-error = Render failed: { $error }
replays-export-open = Open render
replays-export-reset = Reset
replays-export-scale = Scale
replays-export-scale-lossless = lossless
replays-export-disable-bgm = Mute music
replays-export-twosided = Two-sided
replays-export-rounds = Rounds:
replays-export-save-as = Save as…
playback-close = Close
playback-play = Play
playback-pause = Pause
playback-options = Options
playback-speed = Speed
playback-disconnect = Disconnect
playback-disconnect-prompt = Disconnect from this match?
playback-disconnect-detail = You will end the match with your opponent.
playback-cancel = Cancel
replays-select-prompt = Select a replay.
play-opponent = Opponent
replays-match-type = Match type:
replays-duration = Duration:
replays-round-count =
    { $count ->
        [one] 1 round
       *[other] { $count } rounds
    }
replays-incomplete = incomplete
play-you = You
# Patches
patches-update = Update
patches-updating = Updating…
patches-update-failed = Update failed: { $error }
patches-open-folder = Open folder
patches-favorite = Favorite
patches-unfavorite = Unfavorite
patches-search-placeholder = Search patches…
patches-select-prompt = Select a patch.
patches-readme-placeholder = This patch has no README.
patches-details-authors = Authors:
patches-details-license = License:
patches-details-source = Source:
patches-details-games = Supported games:
patches-netplay-compatibility = Netplay compatibility:
# Settings panel
settings-section-general = General
settings-section-graphics = Graphics
settings-section-netplay = Netplay
settings-section-audio = Audio
settings-volume = Volume
settings-nickname = Nickname
settings-language = Language
settings-data-path = Data path
settings-streamer-mode = Streamer privacy mode
settings-section-experimental = Experimental
settings-enable-save-editor = Enable save editor
settings-experimental-warning = Experimental features can break or corrupt your saves, may be changed or removed at any time, and may be missing checks that keep your saves legal for online play. Use them at your own risk.
settings-section-about = About
settings-section-input = Input
settings-input-press-key = Press a key or button…
settings-input-add = Add binding
settings-input-reset = Reset to defaults
input-key-up = Up
input-key-down = Down
input-key-left = Left
input-key-right = Right
input-key-a = A
input-key-b = B
input-key-l = L
input-key-r = R
input-key-start = Start
input-key-select = Select
input-key-speed-up = Fast-forward
input-gamepad-south = Button A
input-gamepad-east = Button B
input-gamepad-west = Button X
input-gamepad-north = Button Y
input-gamepad-select = Select
input-gamepad-start = Start
input-gamepad-mode = Guide
input-gamepad-left-thumb = Left Stick
input-gamepad-right-thumb = Right Stick
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = D-Pad Up
input-gamepad-dpad-down = D-Pad Down
input-gamepad-dpad-left = D-Pad Left
input-gamepad-dpad-right = D-Pad Right
input-gamepad-misc1 = Misc 1
input-gamepad-misc2 = Misc 2
input-gamepad-misc3 = Misc 3
input-gamepad-misc4 = Misc 4
input-gamepad-misc5 = Misc 5
input-gamepad-misc6 = Misc 6
input-gamepad-right-paddle1 = Right Paddle 1
input-gamepad-left-paddle1 = Left Paddle 1
input-gamepad-right-paddle2 = Right Paddle 2
input-gamepad-left-paddle2 = Left Paddle 2
input-gamepad-touchpad = Touchpad
input-gamepad-axis-left-stick-x = Left Stick X
input-gamepad-axis-left-stick-y = Left Stick Y
input-gamepad-axis-right-stick-x = Right Stick X
input-gamepad-axis-right-stick-y = Right Stick Y
input-gamepad-axis-trigger-left = Left Trigger
input-gamepad-axis-trigger-right = Right Trigger
settings-theme = Theme
settings-theme-dark = Dark
settings-theme-light = Light
settings-matchmaking-endpoint = Matchmaking endpoint
settings-patch-repo = Patches repository
settings-enable-patch-autoupdate = Automatically update patches in the background
settings-enable-updater = Automatically check for app updates
settings-allow-prerelease-upgrades = Include prereleases when checking for app updates
settings-netplay-frame-delay = Frame delay
settings-window-size = Window size
settings-fullscreen = Fullscreen
settings-ui-scale = UI scale
settings-video-filter = Video filter
settings-fractional-scaling = Fractional scaling
settings-hide-emulator-border = Hide emulator border
updater-current-version = Current version: { $version }
updater-latest-version = Latest version: { $version }
updater-loading = checking…
updater-up-to-date = v{ $version } (up to date)
updater-downloading = Downloading: { $pct }%
updater-ready-to-update = Update downloaded and ready to install.
updater-update-now = Update now
# Welcome screen
welcome-title = Welcome to Tango!
welcome-subtitle = There's just a few steps you'll need to complete before you can start playing.
welcome-continue = Continue
welcome-step-roms = Add your ROMs
welcome-step-roms-description = Drop your Battle Network / Rockman EXE .gba files into:
welcome-step-roms-detected = { $count } ROMs detected.
welcome-step-nickname = Set your nickname
welcome-step-nickname-description = You can change this at any time in Settings.
welcome-open-folder = Open ROMs folder
welcome-roms-needed = Add at least one ROM before continuing.
# Common actions
rescan = Rescan

# Game names live in games.ftl — same Fluent attribute scheme the
# legacy app uses (game-<family> = base name; .variant-N for each
# regional/colour variant; .match-type-X-Y for per-mode labels).

