# Window
# Endonym for this locale; shown in the language picker.
LANGUAGE = English
window-title = Tango
# Crash handler dialogs (parent process)
crash =
    ¡Oops, Tango ha encontrado un error y se ha estrellado!
    
    Cuando informe de este fallo, incluya el siguiente archivo de registro:
    
    { $path }
crash-no-log =
    ¡Oops, Tango ha encontrado un error y se ha estrellado!
    
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
lobby-input-delay = Input delay (frames)
lobby-input-delay-suggest = Suggest based on ping
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
session-show-opponent = Show opponent
session-hide-opponent = Hide opponent
session-back-to-session = Back to session
# Save view sub-tabs
save-tab-cover = Cover
save-tab-navi = Navi
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-cover-description = This tab intentionally left blank.
# Navi pane
navi-style = Style
# Folder pane
folder-group = Group by chip
save-copy = Copy
save-copy-image = Copy as image
# Navi pane
navi-id = Navi ID
navi-style-unset = (no style)
navicust-grid-size = Grid: { $cols } × { $rows }
navicust-parts = Installed parts
navicust-empty = (none installed)
# Auto Battle Data pane
auto-battle-data-secondary-standard-chips = Standard chips (secondary)
auto-battle-data-standard-chips = Standard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combos
auto-battle-data-program-advance = Program advance
# Common
save-empty = This save has no data for this view.
play-no-selection = Select a game and a save to inspect.
# Replays
replays-filter-all-games = All games
replays-filter-opponent-placeholder = Search opponents…
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
replays-export-scale-na-lossless = (lossless — scale ignored)
replays-export-lossless = Lossless
replays-export-disable-bgm = Mute music
replays-export-twosided = Two-sided
replays-export-rounds = Rounds:
replays-export-save-as = Save as…
playback-close = Close
playback-play = Play
playback-pause = Pause
playback-options = Options
playback-speed = Speed
replays-select-prompt = Select a replay.
replays-opponent = Opponent
replays-match-type = Match type: { $type }
replays-rounds-short =
    { $count ->
        [one] 1 round
       *[other] { $count } rounds
    }
replays-incomplete = incomplete
replays-stats-line = { $duration } · { $rounds }
replays-stats-line-incomplete = { $duration } · { $rounds } · { $incomplete }
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
settings-section-network = Network
settings-nickname = Nickname
settings-language = Language
settings-data-path = Data path
settings-streamer-mode = Streamer privacy mode
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
settings-theme = Theme
settings-matchmaking-endpoint = Matchmaking endpoint
settings-patch-repo = Patches repository
settings-enable-patch-autoupdate = Automatically update patches in the background
settings-enable-updater = Automatically check for app updates
settings-allow-prerelease-upgrades = Include prereleases when checking for app updates
settings-netplay-throttler = Time-sync throttler
settings-video-filter = Video filter
settings-integer-scaling = Integer scaling
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

