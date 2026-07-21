## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = Deutsch
window-quit = Tango beenden
tab-play = Spielen
tab-replays = Wiederholungen
tab-patches = Patches
tab-settings = Einstellungen
play-no-game = Kein Spiel ausgewählt
play-no-save = Speicherstand wählen
play-no-patch = Kein Patch
play-version-placeholder = —
play-link-code = Link-Code (leer lassen für einen zufälligen)
play-play = Spielen
play-fight = Kämpfen!
play-cancel = Abbrechen
play-no-selection = Wähle ein Spiel und einen Speicherstand zur Prüfung aus.
play-status-connecting = Verbinde mit Matchmaking-Server…
play-status-waiting-opponent = Warte auf Gegner…
play-you = Du
play-opponent = Gegner
empty-no-roms-title = Keine Spiel-ROMs gefunden
empty-no-roms-body = Lege deine Battle Network / Rockman EXE .gba-Dateien hier ab:
lobby-waiting = Warten…
lobby-latency = Ping: { $ms } ms
lobby-link-code = Link-Code: { $code }
lobby-match-type = Match-Typ
lobby-ready = Bereit
lobby-unready = Nicht bereit
lobby-match-starting = Beginnt…
lobby-compat-ok = Kompatibel — spielbereit.
lobby-compat-missing-game = Eine Seite hat kein Spiel gewählt.
lobby-compat-missing-rom = Spiel oder Patch ist nicht auf beiden Seiten installiert.
lobby-compat-match-mismatch = Match-Typ stimmt nicht überein.
lobby-pick-game-first = Wähle zuerst ein Spiel
lobby-no-match-types = (keine Match-Typen für dieses Spiel)
session-results-victory = Sieg!
session-results-defeat = Niederlage
session-results-draw = Unentschieden
session-results-no-contest = Match beendet
session-results-vs = gegen { $nickname }
session-results-you = Du
session-results-round = Runde { $number }
session-results-draws = { $count ->
    [one] 1 Runde endete unentschieden
   *[other] { $count } Runden endeten unentschieden
}
session-results-done = Fertig
discord-presence-in-single-player = Im Einzelspieler
discord-presence-in-progress = Spiel im Gange
playback-pause = Pause
playback-close = Schließen
replays-watch = Ansehen
replays-incomplete = unvollständig
replays-watch-missing-rom = Ansehen (ROM für dieses Spiel ist nicht gescannt)
save-delete = Löschen
patches-update = Aktualisierung
patches-updating = Aktualisiere…
patches-update-failed = Aktualisierung fehlgeschlagen: { $error }
patches-netplay-compatibility = Netplay-Kompatibilität:
patches-details-games = Unterstützte Spiele:
patches-select-prompt = Wähle einen Patch.
settings-patch-repo = Patch-Repository
settings-section-general = Allgemein
settings-section-graphics = Grafik
settings-section-audio = Audio
settings-section-input = Eingabe
settings-section-about = Info
settings-volume = Lautstärke
settings-nickname = Nickname
settings-language = Sprache
settings-input-press-key = Drücke eine Taste oder einen Knopf…
settings-input-reset = Auf Standard zurücksetzen
settings-input-select-hint = Klicke auf eine Taste, um ihre Belegung zu bearbeiten
welcome-title = Willkommen bei Tango!
welcome-subtitle = Es gibt nur ein paar Schritte zu erledigen, bevor du spielen kannst.
welcome-continue = Ich bin fertig!
welcome-step-roms = ROMs hinzufügen
welcome-step-roms-detected = { $count } ROMs erkannt.
welcome-step-nickname = Lege deinen Spitznamen fest
welcome-roms-needed = Füge mindestens ein ROM hinzu, um fortzufahren.

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = NaviCust
save-tab-folder = Ordner
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-empty = Dieser Speicherstand hat keine Daten für diese Ansicht.
save-copy = Kopieren
save-copy-image = Als Bild kopieren
copied = Kopiert!
save-edit = Bearbeiten
save-edit-save = Speichern
save-edit-cancel = Abbrechen
save-edit-sort = Sortieren
save-edit-clear = Alle löschen
folder-group = Nach Chip gruppieren
navi-style = Stil
navi-style-unset = (kein Stil)
navi-id = Navi-ID
navi-link-navi = Link-Navi
navi-base-hp = HP
navi-buster = Buster
navi-buster-attack = Angriff
navi-buster-rapid = Rapid
navi-buster-charge = Charge
navi-power-attack = Power-Angriff
navi-edit-select = Navi
folder-edit-search = Chips suchen …
folder-edit-folder = Ordner
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
folder-sort-id = ID
folder-sort-name = Name
folder-sort-code = Code
folder-sort-attack = Angriff
folder-sort-element = Element
folder-sort-mb = MB
navicust-grid-size = Raster: { $cols } × { $rows }
navicust-parts = Installierte Teile
navicust-empty = (keine installiert)
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
auto-battle-data-secondary-standard-chips = Standard Chips (sekundär)
auto-battle-data-standard-chips = Standard Chips
auto-battle-data-mega-chips = Mega Chips
auto-battle-data-giga-chip = Giga Chip
auto-battle-data-combos = Kombos
auto-battle-data-program-advance = Program Advance
auto-battle-data-edit-used = Verwendet
auto-battle-data-edit-secondary = Sek.
auto-battle-data-edit-count = { $count ->
    [one] { $count } Chip
   *[other] { $count } Chips
}
save-actions = Spielstand-Aktionen
save-duplicate = Duplizieren
save-rename = Umbenennen
save-rename-confirm = Umbenennen
save-delete-confirm = Löschen
save-action-cancel = Abbrechen
save-delete-prompt = { $name } löschen?
save-name-placeholder = Neuer Name
save-new = Neuer Speicherstand
save-new-confirm = Erstellen
save-template-default = (Standard)
save-template-pick = Vorlage wählen…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = Alle Spiele
replays-filter-any-time = Beliebige Zeit
replays-filter-past-day = Letzte 24 Stunden
replays-filter-past-week = Letzte Woche
replays-filter-past-month = Letzter Monat
replays-filter-past-year = Letztes Jahr
replays-filter-search-placeholder = Replays durchsuchen…
replays-show-incomplete = Unvollständige anzeigen
replays-direct-marker = (direkt)
replays-select-prompt = Wähle eine Wiederholung.
replays-match-type = Match-Typ:
replays-duration = Dauer:
replays-round-count = { $count ->
    [one] 1 Runde
   *[other] { $count } Runden
}

## patches detail (extracted from the desktop's main.ftl; keep in sync)
patches-favorite = Favorit
patches-unfavorite = Favorit entfernen
patches-search-placeholder = Patches suchen …
patches-readme-placeholder = Dieser Patch hat keine README.
patches-details-authors = Autoren:
patches-details-license = Lizenz:
    .all-rights-reserved = Alle Rechte vorbehalten
patches-details-source = Webseite:

## netplay settings (extracted from the desktop's main.ftl; keep in sync)
settings-matchmaking-endpoint = Matchmaking-Endpunkt
settings-use-relay = Relay-Server verwenden
settings-use-relay-auto = Automatisch
settings-use-relay-always = Immer
settings-use-relay-never = Nie
settings-show-opponent-setup = Setup des Gegners bei Spielbeginn anzeigen

## netplay settings section label (extracted from the desktop)
settings-section-netplay = Netplay

## accent + patch repo settings (extracted from the desktop)
settings-accent = Akzentfarbe
settings-accent-tango-green = Tango-Grün
settings-accent-megaman-blue = MegaMan-Blau
settings-accent-protoman-red = ProtoMan-Rot
settings-accent-roll-pink = Roll-Rosa
settings-accent-gutsman-yellow = GutsMan-Gelb
settings-accent-bass-purple = Bass-Lila

## welcome step description (extracted from the desktop)
welcome-step-nickname-description = Du kannst ihn jederzeit in den Einstellungen ändern.

## replay video export (extracted from the desktop)
replays-export = Export
replays-export-progress = Wird gerendert…
replays-export-cancel = Abbrechen
replays-export-success = Rendern abgeschlossen.
replays-export-error = Rendern fehlgeschlagen: { $error }

## theme + streamer + autoupdate settings (extracted from the desktop)
settings-theme = Theme
settings-theme-dark = Dunkel
settings-theme-light = Hell
settings-streamer-mode = Streamer Privatsphäre-Modus
settings-enable-patch-autoupdate = Aktiviere automatische Aktualisierung

## mute bgm setting (extracted from the desktop)
settings-disable-bgm-in-pvp = Musik im Netplay deaktivieren

## cover tab (extracted from the desktop)
save-tab-cover = Deckel

## lobby + telemetry + transport (extracted from the desktop)
lobby-blind-mine = Setup verbergen
lobby-blind-self-on = Du verbirgst dein Setup.
lobby-blind-peer-on = Gegner verbirgt sein Setup.
settings-netplay-frame-delay = Frame-Verzögerung
lobby-frame-delay-suggest = Anhand des Pings vorschlagen
session-stat-tps = Tick/s (aktuell/max.)
session-stat-skew = Versatz
session-stat-lead = Vorsprung
session-stat-depth = Fehlvorhersagetiefe
session-stat-ping = Netzwerklatenz
playback-speed = Geschwindigkeit
playback-input-display = Eingabeanzeige

## replay input display + swap (extracted from the desktop)
playback-swap-perspective = Perspektive des Gegners

## pvp setup drawers (extracted from the desktop)
session-self = Mein Setup
session-opponent = Gegner-Setup

## replay pip (extracted from the desktop's main.ftl; keep in sync)
playback-pip = Bildschirm des Gegners

## replay transport (extracted from the desktop's main.ftl; keep in sync)
playback-play = Abspielen
playback-clip-tools = Clip
playback-clip-start = Clip-Anfang setzen
playback-clip-end = Clip-Ende setzen
playback-clip-clear = Clip-Markierungen löschen
playback-clip-export = Clip exportieren

## export cancelling (extracted from the desktop's main.ftl; keep in sync)
replays-export-cancelling = Wird abgebrochen…
