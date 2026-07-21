## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = Nederlands
window-quit = Tango afsluiten
tab-play = Spelen
tab-replays = Herhalingen
tab-patches = Patches
tab-settings = Instellingen
play-no-game = No game selected
play-no-save = Save kiezen
play-no-patch = No patch
play-version-placeholder = —
play-link-code = Link code (laat leeg voor een willekeurige)
play-play = Spelen
play-fight = Vechten!
play-cancel = Annuleren
play-no-selection = Kies een spel en save om te bekijken.
play-status-connecting = Verbinden met matchmakingserver…
play-status-waiting-opponent = Wachten op tegenstander…
play-you = Jij
play-opponent = Tegenstander
empty-no-roms-title = Geen ROMs gevonden
empty-no-roms-body = Plaats je Battle Network / Rockman EXE .gba-bestanden in:
lobby-waiting = Wachten…
lobby-latency = Ping: { $ms } ms
lobby-link-code = Link code: { $code }
lobby-match-type = Matchtype
lobby-ready = Klaar
lobby-unready = Niet klaar
lobby-match-starting = Starten…
lobby-compat-ok = Compatibel — klaar om te spelen.
lobby-compat-missing-game = Een kant heeft geen spel gekozen.
lobby-compat-missing-rom = Spel of patch is niet aan beide kanten geïnstalleerd.
lobby-compat-match-mismatch = Matchtype komt niet overeen.
lobby-pick-game-first = Kies eerst een spel
lobby-no-match-types = (geen matchtypes voor dit spel)
session-results-victory = Overwinning!
session-results-defeat = Nederlaag
session-results-draw = Gelijkspel
session-results-no-contest = Match beëindigd
session-results-vs = tegen { $nickname }
session-results-you = Jij
session-results-round = Ronde { $number }
session-results-draws = { $count ->
    [one] 1 ronde eindigde in gelijkspel
   *[other] { $count } rondes eindigden in gelijkspel
}
session-results-done = Klaar
discord-presence-in-single-player = In de singleplayer
discord-presence-in-progress = Spel gaande
playback-pause = Pauzeren
playback-close = Sluiten
replays-watch = Bekijken
replays-incomplete = onvolledig
replays-watch-missing-rom = Bekijken (ROM voor dit spel niet gescand)
save-delete = Verwijderen
patches-update = Bijwerken
patches-updating = Bijwerken…
patches-update-failed = Bijwerken mislukt: { $error }
patches-netplay-compatibility = Netplay-compatibiliteit:
patches-details-games = Ondersteunde spellen:
patches-select-prompt = Selecteer een patch.
settings-patch-repo = Patches repository
settings-section-general = Algemeen
settings-section-graphics = Beeld
settings-section-audio = Geluid
settings-section-input = Invoer
settings-section-about = Info
settings-volume = Volume
settings-nickname = Gebruikersnaam
settings-language = Taal
settings-input-press-key = Druk op een toets of knop…
settings-input-reset = Standaard herstellen
settings-input-select-hint = Klik op een knop om de toewijzingen te bewerken
welcome-title = Welkom bij Tango!
welcome-subtitle = Nog een paar stappen voor je kunt beginnen met spelen.
welcome-continue = Ik ben klaar!
welcome-step-roms = Voeg je ROMs toe
welcome-step-roms-detected = { $count } ROMs gedetecteerd.
welcome-step-nickname = Stel je bijnaam in
welcome-roms-needed = Voeg minstens één ROM toe om door te gaan.

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = NaviCust
save-tab-folder = Map
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-empty = Deze save heeft geen gegevens voor deze weergave.
save-copy = Kopiëren
save-copy-image = Als afbeelding kopiëren
copied = Gekopieerd!
save-edit = Bewerken
save-edit-save = Opslaan
save-edit-cancel = Annuleren
save-edit-sort = Sorteren
save-edit-clear = Alles wissen
folder-group = Groeperen per chip
navi-style = Stijl
navi-style-unset = (geen stijl)
navi-id = Navi-ID
navi-link-navi = Link Navi
navi-base-hp = HP
navi-buster = Buster
navi-buster-attack = Aanval
navi-buster-rapid = Rapid
navi-buster-charge = Charge
navi-power-attack = Krachtaanval
navi-edit-select = Navi
folder-edit-search = Chips zoeken…
folder-edit-folder = Map
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
folder-sort-id = ID
folder-sort-name = Naam
folder-sort-code = Code
folder-sort-attack = Aanval
folder-sort-element = Element
folder-sort-mb = MB
navicust-grid-size = Raster: { $cols } × { $rows }
navicust-parts = Geïnstalleerde onderdelen
navicust-empty = (geen geïnstalleerd)
navicust-edit-grid = NaviCust
navicust-edit-count = { $count ->
    [one] { $count } onderdeel
   *[other] { $count } onderdelen
}
navicust-edit-rotate = Draaien
navicust-edit-compress = Comprimeren
navicust-edit-uncompress = Decomprimeren
navicust-edit-search = Onderdelen zoeken…
navicust-sort-id = ID
navicust-sort-name = Naam
navicust-sort-color = Kleur
patch-card-edit-search = Kaarten zoeken…
patch-card-edit-count = { $count ->
    [one] { $count } kaart
   *[other] { $count } kaarten
}
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = Naam
patch-card-sort-mb = MB
patch-card4-none = Geen
auto-battle-data-secondary-standard-chips = Standaard chips (secundair)
auto-battle-data-standard-chips = Standaard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combo's
auto-battle-data-program-advance = Program advance
auto-battle-data-edit-used = Gebruikt
auto-battle-data-edit-secondary = Sec.
auto-battle-data-edit-count = { $count ->
    [one] { $count } chip
   *[other] { $count } chips
}
save-actions = Save-acties
save-duplicate = Dupliceren
save-rename = Hernoemen
save-rename-confirm = Hernoemen
save-delete-confirm = Verwijderen
save-action-cancel = Annuleren
save-delete-prompt = { $name } verwijderen?
save-name-placeholder = Nieuwe naam
save-new = Nieuwe save
save-new-confirm = Aanmaken
save-template-default = (standaard)
save-template-pick = Kies een sjabloon…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = Alle spellen
replays-filter-any-time = Elk moment
replays-filter-past-day = Afgelopen 24 uur
replays-filter-past-week = Afgelopen week
replays-filter-past-month = Afgelopen maand
replays-filter-past-year = Afgelopen jaar
replays-filter-search-placeholder = Replays zoeken…
replays-show-incomplete = Onvolledige tonen
replays-direct-marker = (direct)
replays-select-prompt = Selecteer een herhaling.
replays-match-type = Matchtype:
replays-duration = Duur:
replays-round-count = { $count ->
    [one] 1 ronde
   *[other] { $count } rondes
}

## patches detail (extracted from the desktop's main.ftl; keep in sync)
patches-favorite = Favoriet
patches-unfavorite = Favoriet verwijderen
patches-search-placeholder = Patches zoeken…
patches-readme-placeholder = Deze patch heeft geen README.
patches-details-authors = Auteurs:
patches-details-license = Licentie:
    .all-rights-reserved = Alle rechten voorbehouden
patches-details-source = Website:

## netplay settings (extracted from the desktop's main.ftl; keep in sync)
settings-matchmaking-endpoint = Eindpunt matchmaking
settings-use-relay = Relayserver gebruiken
settings-use-relay-auto = Automatisch
settings-use-relay-always = Altijd
settings-use-relay-never = Nooit
settings-show-opponent-setup = Opzet van de tegenstander tonen bij het begin van de match

## netplay settings section label (extracted from the desktop)
settings-section-netplay = Netplay

## accent + patch repo settings (extracted from the desktop)
settings-accent = Accentkleur
settings-accent-tango-green = Tango-groen
settings-accent-megaman-blue = MegaMan-blauw
settings-accent-protoman-red = ProtoMan-rood
settings-accent-roll-pink = Roll-roze
settings-accent-gutsman-yellow = GutsMan-geel
settings-accent-bass-purple = Bass-paars

## welcome step description (extracted from the desktop)
welcome-step-nickname-description = Je kunt deze altijd wijzigen via Instellingen.

## replay video export (extracted from the desktop)
replays-export = Exporteren
replays-export-progress = Renderen…
replays-export-cancel = Annuleren
replays-export-success = Renderen voltooid.
replays-export-error = Renderen mislukt: { $error }

## theme + streamer + autoupdate settings (extracted from the desktop)
settings-theme = Thema
settings-theme-dark = Donker
settings-theme-light = Licht
settings-streamer-mode = Streamer privacymodus
settings-enable-patch-autoupdate = Autoupdate inschakelen

## mute bgm setting (extracted from the desktop)
settings-disable-bgm-in-pvp = Muziek in netplay uitschakelen

## cover tab (extracted from the desktop)
save-tab-cover = Omslag

## lobby + telemetry + transport (extracted from the desktop)
lobby-blind-mine = Opzet verbergen
lobby-blind-self-on = Je verbergt je opzet.
lobby-blind-peer-on = Tegenstander verbergt zijn opzet.
settings-netplay-frame-delay = Framevertraging
lobby-frame-delay-suggest = Voorstellen op basis van ping
session-stat-tps = Tick/s (huidig/max.)
session-stat-skew = Afwijking
session-stat-lead = Voorsprong
session-stat-depth = Misvoorspellingsdiepte
session-stat-ping = Netwerklatentie
playback-speed = Snelheid
playback-input-display = Invoerweergave

## replay input display + swap (extracted from the desktop)
playback-swap-perspective = Perspectief van tegenstander

## pvp setup drawers (extracted from the desktop)
session-self = Mijn opzet
session-opponent = Opzet van tegenstander
