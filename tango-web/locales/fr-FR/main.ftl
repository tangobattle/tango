## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = Français
window-quit = Quitter Tango
tab-play = Jouer
tab-replays = Replays
tab-patches = Patches
tab-settings = Paramètres
play-no-game = Aucun jeu sélectionné
play-no-save = Choisir une sauvegarde
play-no-patch = Aucun patch
play-version-placeholder = —
play-link-code = Code de connexion (laisser vide pour un code aléatoire)
play-play = Jouer
play-fight = Bagarre !
play-cancel = Annuler
play-no-selection = Choisissez un jeu et une sauvegarde à inspecter.
play-status-connecting = Connexion au serveur de matchmaking…
play-status-waiting-opponent = En attente de l'adversaire…
play-you = Vous-même
play-opponent = Adversaire
empty-no-roms-title = Aucune ROM trouvée
empty-no-roms-body = Placez vos fichiers .gba de Battle Network / Rockman EXE dans :
lobby-waiting = En attente…
lobby-latency = Ping : { $ms } ms
lobby-link-code = Code de connexion : { $code }
lobby-match-type = Type de match
lobby-ready = Prêt
lobby-unready = Pas prêt
lobby-match-starting = Démarrage…
lobby-compat-ok = Compatible — prêt à jouer.
lobby-compat-missing-game = Un côté n'a pas choisi de jeu.
lobby-compat-missing-rom = Le jeu ou le patch n'est pas installé des deux côtés.
lobby-compat-match-mismatch = Le type de match ne correspond pas.
lobby-pick-game-first = Choisissez d'abord un jeu
lobby-no-match-types = (aucun type de match pour ce jeu)
session-results-victory = Victoire !
session-results-defeat = Défaite
session-results-draw = Égalité
session-results-no-contest = Match terminé
session-results-vs = contre { $nickname }
session-results-you = Vous
session-results-round = Round { $number }
session-results-draws = { $count ->
    [one] 1 round s'est terminé par une égalité
   *[other] { $count } rounds se sont terminés par une égalité
}
session-results-done = Terminé
discord-presence-in-single-player = En mode solo
discord-presence-in-progress = Match en cours
playback-pause = Pause
playback-close = Fermer
replays-watch = Regarder
replays-incomplete = incomplet
replays-watch-missing-rom = Regarder (ROM de ce jeu non analysée)
save-delete = Supprimer
patches-update = Mettre à jour
patches-updating = Mise à jour…
patches-update-failed = Échec de la mise à jour: { $error }
patches-netplay-compatibility = Compatibilité netplay :
patches-details-games = Jeux pris en charge :
patches-select-prompt = Sélectionnez un patch.
settings-patch-repo = Dépôt des patchs
settings-section-general = Général
settings-section-graphics = Graphismes
settings-section-audio = Audio
settings-section-input = Entrée
settings-section-about = À propos
settings-volume = Volume
settings-nickname = Surnom
settings-language = Langue
settings-input-press-key = Appuyez sur une touche ou un bouton…
settings-input-reset = Restaurer les valeurs par défaut
settings-input-select-hint = Cliquez sur un bouton pour modifier ses touches
welcome-title = Bienvenue dans Tango !
welcome-subtitle = Encore quelques étapes avant de pouvoir jouer.
welcome-continue = J'ai terminé !
welcome-step-roms = Ajoutez vos ROMs
welcome-step-roms-detected = { $count } ROMs détectées.
welcome-step-nickname = Choisissez votre pseudo
welcome-roms-needed = Ajoutez au moins une ROM pour continuer.

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = NaviCust
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-empty = Cette sauvegarde n'a pas de données pour cette vue.
save-copy = Copier
save-copy-image = Copier en image
copied = Copié !
save-edit = Modifier
save-edit-save = Enregistrer
save-edit-cancel = Annuler
save-edit-sort = Trier
save-edit-clear = Tout effacer
folder-group = Grouper par puce
navi-style = Style
navi-style-unset = (aucun style)
navi-id = ID du Navi
navi-link-navi = Link Navi
navi-base-hp = HP
navi-buster = Buster
navi-buster-attack = Attaque
navi-buster-rapid = Cadence
navi-buster-charge = Charge
navi-power-attack = Attaque puissante
navi-edit-select = Navi
folder-edit-search = Rechercher des puces…
folder-edit-folder = Folder
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
folder-sort-id = ID
folder-sort-name = Nom
folder-sort-code = Code
folder-sort-attack = Attaque
folder-sort-element = Élément
folder-sort-mb = MB
navicust-grid-size = Grille: { $cols } × { $rows }
navicust-parts = Pièces installées
navicust-empty = (aucune installée)
navicust-edit-grid = NaviCust
navicust-edit-count = { $count ->
    [one] { $count } pièce
   *[other] { $count } pièces
}
navicust-edit-rotate = Pivoter
navicust-edit-compress = Compresser
navicust-edit-uncompress = Décompresser
navicust-edit-search = Rechercher des pièces…
navicust-sort-id = ID
navicust-sort-name = Nom
navicust-sort-color = Couleur
patch-card-edit-search = Rechercher des cartes…
patch-card-edit-count = { $count ->
    [one] { $count } carte
   *[other] { $count } cartes
}
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = Nom
patch-card-sort-mb = MB
patch-card4-none = Aucune
auto-battle-data-secondary-standard-chips = Standard chips (secondaire)
auto-battle-data-standard-chips = Standard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combos
auto-battle-data-program-advance = Program advance
auto-battle-data-edit-used = Usages
auto-battle-data-edit-secondary = Sec.
auto-battle-data-edit-count = { $count ->
    [one] { $count } puce
   *[other] { $count } puces
}
save-actions = Actions de sauvegarde
save-duplicate = Dupliquer
save-rename = Renommer
save-rename-confirm = Renommer
save-delete-confirm = Supprimer
save-action-cancel = Annuler
save-delete-prompt = Supprimer { $name } ?
save-name-placeholder = Nouveau nom
save-new = Nouvelle sauvegarde
save-new-confirm = Créer
save-template-default = (par défaut)
save-template-pick = Choisir un modèle…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = Tous les jeux
replays-filter-any-time = Toute période
replays-filter-past-day = Dernières 24 heures
replays-filter-past-week = Semaine passée
replays-filter-past-month = Mois passé
replays-filter-past-year = Année passée
replays-filter-search-placeholder = Rechercher des replays…
replays-show-incomplete = Afficher incomplets
replays-direct-marker = (direct)
replays-select-prompt = Sélectionnez un replay.
replays-match-type = Type de match :
replays-duration = Durée :
replays-round-count = { $count ->
    [one] 1 round
   *[other] { $count } rounds
}

## patches detail (extracted from the desktop's main.ftl; keep in sync)
patches-favorite = Favori
patches-unfavorite = Retirer des favoris
patches-search-placeholder = Rechercher des patchs…
patches-readme-placeholder = Ce patch n'a pas de README.
patches-details-authors = Auteurs :
patches-details-license = Licence :
    .all-rights-reserved = Tous les droits sont réservés
patches-details-source = Site Web :

## netplay settings (extracted from the desktop's main.ftl; keep in sync)
settings-matchmaking-endpoint = Point d'arrivée de matchmaking
settings-use-relay = Utiliser le serveur relais
settings-use-relay-auto = Auto
settings-use-relay-always = Toujours
settings-use-relay-never = Jamais
settings-show-opponent-setup = Afficher la configuration de l'adversaire au début du match

## netplay settings section label (extracted from the desktop)
settings-section-netplay = Netplay

## accent + patch repo settings (extracted from the desktop)
settings-accent = Couleur d'accent
settings-accent-tango-green = Vert Tango
settings-accent-megaman-blue = Bleu MegaMan
settings-accent-protoman-red = Rouge ProtoMan
settings-accent-roll-pink = Rose Roll
settings-accent-gutsman-yellow = Jaune GutsMan
settings-accent-bass-purple = Violet Bass

## welcome step description (extracted from the desktop)
welcome-step-nickname-description = Vous pourrez le changer à tout moment dans les Paramètres.

## replay video export (extracted from the desktop)
replays-export = Exporter
replays-export-progress = Rendu en cours…
replays-export-cancel = Annuler
replays-export-success = Rendu terminé.
replays-export-error = Échec du rendu: { $error }

## theme + streamer + autoupdate settings (extracted from the desktop)
settings-theme = Thème
settings-theme-dark = Sombre
settings-theme-light = Clair
settings-streamer-mode = Mode de confidentialité du Streamer
settings-enable-patch-autoupdate = Activer la mise à jour automatique

## mute bgm setting (extracted from the desktop)
settings-disable-bgm-in-pvp = Désactiver la musique pendant le netplay

## cover tab (extracted from the desktop)
save-tab-cover = Couverture

## lobby + telemetry + transport (extracted from the desktop)
lobby-blind-mine = Cacher ma configuration
lobby-blind-self-on = Vous cachez votre configuration.
lobby-blind-peer-on = L'adversaire cache sa configuration.
settings-netplay-frame-delay = Délai d'image
lobby-frame-delay-suggest = Suggérer selon le ping
session-stat-tps = Tick/s (actuel/max)
session-stat-skew = Décalage
session-stat-lead = Avance
session-stat-depth = Profondeur de prédiction erronée
session-stat-ping = Latence réseau
playback-speed = Vitesse
playback-input-display = Affichage des entrées

## replay input display + swap (extracted from the desktop)
playback-swap-perspective = Perspective de l'adversaire

## pvp setup drawers (extracted from the desktop)
session-self = Ma configuration
session-opponent = Configuration de l'adversaire
