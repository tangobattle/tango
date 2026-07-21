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
session-results-done = Done
discord-presence-in-single-player = In single player
discord-presence-in-progress = Match in progress
playback-pause = Pause
playback-close = Close
replays-watch = Watch
replays-incomplete = incomplete
replays-watch-missing-rom = Watch (ROM for this game isn't scanned)
save-delete = Delete
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
