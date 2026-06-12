# Endonym for this locale; shown in the language picker.
LANGUAGE = Deutsch

crash =
    Oops, Tango ist auf einen Fehler gestoßen und ist abgestürzt!

    Wenn Sie diesen Absturz melden, fügen Sie bitte die folgende Protokolldatei bei:

    { $path }
crash-no-log =
    Oops, Tango ist auf einen Fehler gestoßen und abgestürzt!

    { $error }
window-title = Tango
    .running = Tango (läuft)
play-play = Spielen
play-fight = Kämpfen!
play-link-code = Link-Code (leer lassen für einen zufälligen)
play-no-game = Kein Spiel ausgewählt
play-no-patch = Kein Patch
play-patch-toggle = Patch verwenden…
play-you = Du
play-cancel = Abbrechen
replays-export = Export
replays-export-disable-bgm = Musik deaktivieren
replays-export-twosided = Beidseitig
replays-export-success = Rendern abgeschlossen.
replays-export-error = Rendern fehlgeschlagen: { $error }
replays-export-open = Öffnen
patches-open-folder = Ordner öffnen
patches-favorite = Favorit
patches-unfavorite = Favorit entfernen
patches-search-placeholder = Patches suchen …
patches-update = Aktualisierung
patches-details-authors = Autoren:
patches-details-license = Lizenz:
    .all-rights-reserved = Alle Rechte vorbehalten
patches-details-source = Webseite:
patches-details-games = Unterstützte Spiele:
settings-theme = Theme
settings-theme-dark = Dunkel
settings-theme-light = Hell
    .light = Hell
    .dark = Dunkel
    .system = Systemeinstellung befolgen
settings-video-filter = Video-Filter
    .null = Keine
    .hq2x = hq2x
    .hq3x = hq3x
    .hq4x = hq4x
    .mmpx = MMPX
settings-language = Sprache
settings-nickname = Nickname
settings-streamer-mode = Streamer Privatsphäre-Modus
settings-section-experimental = Experimentell
settings-enable-save-editor = Speicherstand-Editor aktivieren
settings-experimental-warning = Experimentelle Funktionen können deine Speicherstände beschädigen oder unbrauchbar machen, können jederzeit geändert oder entfernt werden und es fehlen ihnen unter Umständen Prüfungen, die deine Speicherstände für das Online-Spiel zulässig halten. Nutzung auf eigene Gefahr.
    .tooltip = Bei Aktivierung des Modus, wird der Speicheranzeige ein zusätzliches Abdeckungsfenster hinzugefügt, die alle Informationen über Ihre aktuelle Speicherdatei verbirgt.
settings-matchmaking-endpoint = Matchmaking-Endpunkt
settings-patch-repo = Patch-Repository
settings-enable-patch-autoupdate = Aktiviere automatische Aktualisierung
settings-data-path = Datenpfad
    .open = Öffnen
    .change = Ändern
settings-window-size = Fenstergröße
settings-fullscreen = Vollbild
settings-ui-scale = UI-Skalierung
settings-fractional-scaling = Fraktionale Skalierung
settings-hide-emulator-border = Emulator-Rahmen ausblenden
save-tab-cover = Deckel
save-tab-navi = Navi
save-tab-folder = Ordner
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
auto-battle-data-secondary-standard-chips = Standard Chips (sekundär)
auto-battle-data-standard-chips = Standard Chips
auto-battle-data-mega-chips = Mega Chips
auto-battle-data-giga-chip = Giga Chip
auto-battle-data-combos = Kombos
auto-battle-data-program-advance = Program Advance

# Auto Battle Data editor
auto-battle-data-edit-used = Verwendet
auto-battle-data-edit-secondary = Sek.
auto-battle-data-edit-count = { $count ->
    [one] { $count } Chip
   *[other] { $count } Chips
}
welcome-open-folder = Ordner öffnen
welcome-continue = Ich bin fertig!
discord-presence-looking = Suche nach einem Kampf
discord-presence-in-single-player = Im Einzelspieler
discord-presence-in-lobby = In der Lobby
discord-presence-in-progress = Spiel im Gange

# === translations ===
tab-play = Spielen
tab-replays = Wiederholungen
tab-patches = Patches
tab-settings = Einstellungen
play-no-save = Speicherstand wählen
save-open-folder = Ordner öffnen
save-duplicate = Duplizieren
save-rename = Umbenennen
save-delete = Löschen
save-rename-confirm = Umbenennen
save-delete-confirm = Löschen
save-action-cancel = Abbrechen
save-delete-prompt = { $name } löschen?
save-name-placeholder = Neuer Name
save-new = Neuer Speicherstand
save-new-confirm = Erstellen
save-template-default = (Standard)
save-template-pick = Vorlage wählen…
empty-no-roms-title = Keine Spiel-ROMs gefunden
empty-no-roms-body = Lege deine Battle Network / Rockman EXE .gba-Dateien hier ab:
empty-no-saves-title = Keine Speicherstände für dieses Spiel
empty-no-saves-body = Lege eine .sav-Datei für dieses Spiel hier ab:
play-version-placeholder = —
play-link-code-random = Zufälliger Link-Code
play-status-idle = Gib einen Link-Code für Netplay ein oder lass das Feld leer für Einzelspieler.
play-status-connecting = Verbinde mit Matchmaking-Server…
play-status-direct-connecting = Verbinde mit Gegner…
play-status-waiting-opponent = Warte auf Gegner…
play-status-negotiating = Verhandle…
play-status-failed = Verbindung fehlgeschlagen: { $error }
play-status-peer-disconnected = Der andere Spieler hat das Spiel verlassen.
play-status-negotiate-expected-hello = Der andere Spieler hat den erwarteten Handshake nicht gesendet.
play-status-negotiate-version-too-old = Der andere Spieler verwendet eine ältere Version von Tango.
play-status-negotiate-version-too-new = Der andere Spieler verwendet eine neuere Version von Tango.
play-status-negotiate-failed = Bei der Verhandlung ist ein Fehler aufgetreten: { $error }
lobby-waiting = Warten…
lobby-no-game = (kein Spiel gewählt)
lobby-latency = Ping: { $ms } ms
lobby-link-code = Link-Code: { $code }
lobby-direct-host = Hosten auf Port: { $port }
lobby-direct-connect = Verbinde mit: { $target }
lobby-handshake = Tausche Einstellungen aus…
lobby-match-type = Match-Typ
settings-netplay-frame-delay = Frame-Verzögerung
settings-use-relay = Relay-Server verwenden
settings-use-relay-auto = Automatisch
settings-use-relay-always = Immer
settings-use-relay-never = Nie
lobby-frame-delay-suggest = Anhand des Pings vorschlagen
lobby-no-match-types = (keine Match-Typen für dieses Spiel)
lobby-pick-game-first = Wähle zuerst ein Spiel
lobby-compat-ok = Kompatibel — spielbereit.
lobby-compat-missing-game = Eine Seite hat kein Spiel gewählt.
lobby-compat-missing-rom = Spiel oder Patch ist nicht auf beiden Seiten installiert.
lobby-compat-version-mismatch = Spielversionen stimmen nicht überein (anderer Patch / ROM).
lobby-compat-match-mismatch = Match-Typ stimmt nicht überein.
lobby-ready = Bereit
lobby-unready = Nicht bereit
lobby-match-starting = Beginnt…
lobby-blind-mine = Setup verbergen
lobby-blind-peer-on = Gegner verbirgt sein Setup.
lobby-blind-self-on = Du verbirgst dein Setup.
session-opponent = Gegner-Setup
session-self = Mein Setup
session-back-to-session = Zurück zur Sitzung
# PvP telemetry deck cell tooltips
session-stat-tps = Tick/s (aktuell/max.)
session-stat-skew = Versatz
session-stat-depth = Fehlvorhersagetiefe
session-stat-ping = Netzwerklatenz
navi-style = Stil
folder-group = Nach Chip gruppieren
save-copy = Kopieren
copied = Kopiert!
save-copy-image = Als Bild kopieren
navi-id = Navi-ID
navi-link-navi = Link-Navi
navi-style-unset = (kein Stil)
navicust-grid-size = Raster: { $cols } × { $rows }
navicust-parts = Installierte Teile
navicust-empty = (keine installiert)

# Folder editor
save-edit = Bearbeiten
save-edit-save = Speichern
save-edit-cancel = Abbrechen
folder-edit-search = Chips suchen …
folder-edit-folder = Ordner
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
save-edit-sort = Sortieren
save-edit-clear = Alle löschen
folder-sort-id = ID
folder-sort-name = Name
folder-sort-code = Code
folder-sort-attack = Angriff
folder-sort-element = Element
folder-sort-mb = MB

# Navicust editor
navicust-edit-grid = NaviCust
navicust-edit-count = { $count ->
    [one] { $count } Teil
   *[other] { $count } Teile
}
navicust-edit-rotate = Drehen
navicust-edit-compress = Komprimieren
navicust-edit-uncompress = Dekomprimieren
navicust-edit-search = Teile suchen…
navicust-sort-id = ID
navicust-sort-name = Name
navicust-sort-color = Farbe

# Patch card editor
patch-card-edit-search = Karten suchen …
patch-card-edit-count = { $count ->
    [one] { $count } Karte
   *[other] { $count } Karten
}
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = Name
patch-card-sort-mb = MB
patch-card4-none = Keine
save-empty = Dieser Speicherstand hat keine Daten für diese Ansicht.
play-no-selection = Wähle ein Spiel und einen Speicherstand zur Prüfung aus.
replays-filter-all-games = Alle Spiele
replays-filter-opponent-placeholder = Beliebig
replays-show-incomplete = Unvollständige anzeigen
replays-direct-marker = (direkt)
replays-watch = Ansehen
replays-watch-missing-rom = Ansehen (ROM für dieses Spiel ist nicht gescannt)
replays-export-progress = Wird gerendert…
replays-export-cancel = Abbrechen
replays-export-cancelling = Wird abgebrochen…
replays-export-reset = Zurücksetzen
replays-export-scale = Skalierung
replays-export-scale-lossless = verlustfrei
replays-export-rounds = Runden:
replays-export-save-as = Speichern unter…
playback-close = Schließen
playback-options = Optionen
playback-speed = Geschwindigkeit
playback-play = Abspielen
playback-pause = Pause
playback-disconnect = Trennen
playback-disconnect-prompt = Verbindung zu dieser Partie trennen?
playback-disconnect-detail = Du beendest damit die Partie mit deinem Gegner.
playback-cancel = Abbrechen
replays-select-prompt = Wähle eine Wiederholung.
play-opponent = Gegner
replays-match-type = Match-Typ:
replays-duration = Dauer:
replays-round-count = { $count ->
    [one] 1 Runde
   *[other] { $count } Runden
}
replays-incomplete = unvollständig
patches-updating = Aktualisiere…
patches-update-failed = Aktualisierung fehlgeschlagen: { $error }
patches-select-prompt = Wähle einen Patch.
patches-readme-placeholder = Dieser Patch hat keine README.
patches-netplay-compatibility = Netplay-Kompatibilität:
settings-section-general = Allgemein
settings-section-graphics = Grafik
settings-section-netplay = Netplay
settings-section-audio = Audio
settings-volume = Lautstärke
settings-disable-bgm-in-pvp = Musik im Netplay deaktivieren
settings-section-about = Info
settings-section-input = Eingabe
settings-input-press-key = Drücke eine Taste oder einen Knopf…
settings-input-add = Belegung hinzufügen
settings-input-reset = Auf Standard zurücksetzen
input-key-up = Hoch
input-key-down = Runter
input-key-left = Links
input-key-right = Rechts
input-key-a = A
input-key-b = B
input-key-l = L
input-key-r = R
input-key-start = Start
input-key-select = Select
input-key-speed-up = Schnellvorlauf
input-gamepad-south = A-Taste
input-gamepad-east = B-Taste
input-gamepad-west = X-Taste
input-gamepad-north = Y-Taste
input-gamepad-select = Select
input-gamepad-start = Start
input-gamepad-mode = Guide
input-gamepad-left-thumb = Linker Stick
input-gamepad-right-thumb = Rechter Stick
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = Steuerkreuz hoch
input-gamepad-dpad-down = Steuerkreuz runter
input-gamepad-dpad-left = Steuerkreuz links
input-gamepad-dpad-right = Steuerkreuz rechts
input-gamepad-misc1 = Sonstige 1
input-gamepad-misc2 = Sonstige 2
input-gamepad-misc3 = Sonstige 3
input-gamepad-misc4 = Sonstige 4
input-gamepad-misc5 = Sonstige 5
input-gamepad-misc6 = Sonstige 6
input-gamepad-right-paddle1 = Rechtes Paddle 1
input-gamepad-left-paddle1 = Linkes Paddle 1
input-gamepad-right-paddle2 = Rechtes Paddle 2
input-gamepad-left-paddle2 = Linkes Paddle 2
input-gamepad-touchpad = Touchpad
input-gamepad-axis-left-stick-x = Linker Stick X
input-gamepad-axis-left-stick-y = Linker Stick Y
input-gamepad-axis-right-stick-x = Rechter Stick X
input-gamepad-axis-right-stick-y = Rechter Stick Y
input-gamepad-axis-trigger-left = Linker Trigger
input-gamepad-axis-trigger-right = Rechter Trigger
settings-enable-updater = Automatisch nach App-Updates suchen
settings-allow-prerelease-upgrades = Vorabversionen bei Update-Suche einbeziehen
updater-current-version = Aktuelle Version: { $version }
updater-latest-version = Neueste Version: { $version }
updater-loading = wird geprüft…
updater-up-to-date = v{ $version } (aktuell)
updater-downloading = Wird heruntergeladen: { $pct }%
updater-ready-to-update = Update heruntergeladen und bereit zur Installation.
updater-update-now = Jetzt aktualisieren
welcome-title = Willkommen bei Tango!
welcome-subtitle = Es gibt nur ein paar Schritte zu erledigen, bevor du spielen kannst.
welcome-step-roms = ROMs hinzufügen
welcome-step-roms-description = Lege deine Battle Network / Rockman EXE .gba-Dateien hier ab:
welcome-step-roms-detected = { $count } ROMs erkannt.
welcome-step-nickname = Lege deinen Spitznamen fest
welcome-step-nickname-description = Du kannst ihn jederzeit in den Einstellungen ändern.
welcome-roms-needed = Füge mindestens ein ROM hinzu, um fortzufahren.
rescan = Erneut suchen
