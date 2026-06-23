# Endonym for this locale; shown in the language picker.
LANGUAGE = Français

crash =
    Oups, Tango a rencontré une erreur et a planté !

    Lorsque vous signalez ce plantage, veuillez inclure le log suivant :

    { $path }
crash-no-log =
    Oups, Tango a rencontré une erreur et a planté !

    { $error }
window-title = Tango
    .running = Tango (en cours d'exécution)
# Tooltip on the top bar's close button (fullscreen only).
window-quit = Quitter Tango
play-play = Jouer
play-fight = Bagarre !
play-link-code = Code de connexion (laisser vide pour un code aléatoire)
play-no-game = Aucun jeu sélectionné
play-no-patch = Aucun patch
play-patch-toggle = Utiliser un patch…
play-you = Vous-même
play-cancel = Annuler
replays-export = Exporter
replays-export-disable-bgm = Désactiver la musique
replays-export-twosided = Deux côtés
replays-export-success = Rendu terminé.
replays-export-error = Échec du rendu: { $error }
replays-export-open = Ouvrir
patches-open-folder = Ouvrir le dossier
patches-favorite = Favori
patches-unfavorite = Retirer des favoris
patches-search-placeholder = Rechercher des patchs…
patches-update = Mettre à jour
patches-details-authors = Auteurs :
patches-details-license = Licence :
    .all-rights-reserved = Tous les droits sont réservés
patches-details-source = Site Web :
patches-details-games = Jeux pris en charge :
settings-theme = Thème
settings-theme-dark = Sombre
settings-theme-light = Clair
    .light = Clair
    .dark = Sombre
    .system = Suivre le réglage du système
settings-video-filter = Filtres vidéo
    .null = Aucun
    .hq2x = hq2x
    .hq3x = hq3x
    .hq4x = hq4x
    .mmpx = MMPX
settings-language = Langue
settings-nickname = Surnom
settings-streamer-mode = Mode de confidentialité du Streamer
settings-section-experimental = Expérimental
settings-enable-save-editor = Activer l'éditeur de sauvegarde
settings-experimental-warning = Les fonctionnalités expérimentales peuvent casser ou corrompre vos sauvegardes, peuvent être modifiées ou supprimées à tout moment et peuvent ne pas comporter les vérifications qui gardent vos sauvegardes valides pour le jeu en ligne. À utiliser à vos propres risques.
    .tooltip = Activer ce mode ajoutera un onglet "Couverture" supplémentaire à la visionneuse de sauvegarde qui masquera toutes les informations sur votre fichier de sauvegarde actuel.
settings-matchmaking-endpoint = Point d'arrivée de matchmaking
settings-patch-repo = Dépôt des patchs
settings-enable-patch-autoupdate = Activer la mise à jour automatique
settings-data-path = Chemin des données
    .open = Ouvrir
    .change = Changer
settings-window-size = Taille de la fenêtre
settings-fullscreen = Plein écran
settings-ui-scale = Échelle de l'interface
settings-fractional-scaling = Mise à l'échelle fractionnaire
settings-hide-emulator-border = Masquer la bordure de l'émulateur
save-tab-cover = Couverture
save-tab-navi = Navi
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
auto-battle-data-secondary-standard-chips = Standard chips (secondaire)
auto-battle-data-standard-chips = Standard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combos
auto-battle-data-program-advance = Program advance

# Auto Battle Data editor
auto-battle-data-edit-used = Usages
auto-battle-data-edit-secondary = Sec.
auto-battle-data-edit-count = { $count ->
    [one] { $count } puce
   *[other] { $count } puces
}
welcome-open-folder = Ouvrir le dossier
welcome-continue = J'ai terminé !
discord-presence-looking = À la recherche d'un match
discord-presence-in-single-player = En mode solo
discord-presence-in-lobby = Dans un lobby
discord-presence-in-progress = Match en cours

# === translations ===
tab-play = Jouer
tab-replays = Replays
tab-patches = Patches
tab-settings = Paramètres
play-no-save = Choisir une sauvegarde
save-open-folder = Ouvrir le dossier
save-duplicate = Dupliquer
save-rename = Renommer
save-delete = Supprimer
save-rename-confirm = Renommer
save-delete-confirm = Supprimer
save-action-cancel = Annuler
save-delete-prompt = Supprimer { $name } ?
save-name-placeholder = Nouveau nom
save-new = Nouvelle sauvegarde
save-new-confirm = Créer
save-template-default = (par défaut)
save-template-pick = Choisir un modèle…
empty-no-roms-title = Aucune ROM trouvée
empty-no-roms-body = Placez vos fichiers .gba de Battle Network / Rockman EXE dans :
empty-no-saves-title = Aucune sauvegarde pour ce jeu
empty-no-saves-body = Placez un fichier .sav pour ce jeu dans :
play-version-placeholder = —
play-link-code-random = Code aléatoire
play-status-idle = Entrez un code pour démarrer le netplay, ou laissez vide pour le solo.
play-status-connecting = Connexion au serveur de matchmaking…
play-status-direct-connecting = Connexion à l'adversaire…
play-status-waiting-opponent = En attente de l'adversaire…
play-status-negotiating = Négociation…
play-status-failed = Échec de la connexion: { $error }
play-status-peer-disconnected = L'autre joueur a quitté la partie.
play-status-negotiate-expected-hello = L'autre joueur n'a pas envoyé la poignée de main attendue.
play-status-negotiate-version-too-old = L'autre joueur utilise une version plus ancienne de Tango.
play-status-negotiate-version-too-new = L'autre joueur utilise une version plus récente de Tango.
play-status-negotiate-failed = Une erreur s'est produite lors de la négociation : { $error }
lobby-waiting = En attente…
lobby-no-game = (aucun jeu choisi)
lobby-latency = Ping : { $ms } ms
lobby-latency-direct = Ping (direct) : { $ms } ms
lobby-latency-relayed = Ping (relayé) : { $ms } ms
lobby-link-code = Code de connexion : { $code }
lobby-direct-host = Hébergement sur le port UDP : { $port }
lobby-direct-connect = Connexion via UDP : { $target }
lobby-handshake = Échange des paramètres…
lobby-match-type = Type de match
settings-netplay-frame-delay = Délai d'image
settings-use-relay = Utiliser le serveur relais
settings-use-relay-auto = Auto
settings-use-relay-always = Toujours
settings-use-relay-never = Jamais
lobby-frame-delay-suggest = Suggérer selon le ping
lobby-no-match-types = (aucun type de match pour ce jeu)
lobby-pick-game-first = Choisissez d'abord un jeu
lobby-compat-ok = Compatible — prêt à jouer.
lobby-compat-missing-game = Un côté n'a pas choisi de jeu.
lobby-compat-missing-rom = Le jeu ou le patch n'est pas installé des deux côtés.
lobby-compat-version-mismatch = Les versions du jeu diffèrent (patch / ROM différents).
lobby-compat-match-mismatch = Le type de match ne correspond pas.
lobby-ready = Prêt
lobby-unready = Pas prêt
lobby-match-starting = Démarrage…
lobby-blind-mine = Cacher ma configuration
lobby-blind-peer-on = L'adversaire cache sa configuration.
lobby-blind-self-on = Vous cachez votre configuration.
session-opponent = Configuration de l'adversaire
session-self = Ma configuration
session-back-to-session = Retour à la session
# PvP telemetry deck cell tooltips
session-stat-tps = Tick/s (actuel/max)
session-stat-skew = Décalage
session-stat-depth = Profondeur de prédiction erronée
session-stat-ping = Latence réseau
navi-style = Style
folder-group = Grouper par puce
save-copy = Copier
copied = Copié !
save-copy-image = Copier en image
navi-id = ID du Navi
navi-link-navi = Link Navi
navi-style-unset = (aucun style)
navicust-grid-size = Grille: { $cols } × { $rows }
navicust-parts = Pièces installées
navicust-empty = (aucune installée)

# Folder editor
save-edit = Modifier
save-edit-save = Enregistrer
save-edit-cancel = Annuler
folder-edit-search = Rechercher des puces…
folder-edit-folder = Folder
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
save-edit-sort = Trier
save-edit-clear = Tout effacer
folder-sort-id = ID
folder-sort-name = Nom
folder-sort-code = Code
folder-sort-attack = Attaque
folder-sort-element = Élément
folder-sort-mb = MB

# Navicust editor
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

# Patch card editor
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
save-empty = Cette sauvegarde n'a pas de données pour cette vue.
play-no-selection = Choisissez un jeu et une sauvegarde à inspecter.
replays-filter-all-games = Tous les jeux
replays-filter-opponent-placeholder = Tous
replays-show-incomplete = Afficher incomplets
replays-direct-marker = (direct)
replays-watch = Regarder
replays-watch-missing-rom = Regarder (ROM de ce jeu non analysée)
replays-export-progress = Rendu en cours…
replays-export-cancel = Annuler
replays-export-cancelling = Annulation…
replays-export-reset = Réinitialiser
replays-export-scale = Échelle
replays-export-scale-lossless = sans perte
replays-export-rounds = Rounds :
replays-export-save-as = Enregistrer sous…
playback-close = Fermer
playback-options = Options
playback-speed = Vitesse
playback-play = Lire
playback-pause = Pause
playback-disconnect = Se déconnecter
playback-disconnect-prompt = Se déconnecter de ce match ?
playback-disconnect-detail = Vous mettrez fin au match avec votre adversaire.
playback-cancel = Annuler
replays-select-prompt = Sélectionnez un replay.
play-opponent = Adversaire
replays-match-type = Type de match :
replays-duration = Durée :
replays-round-count = { $count ->
    [one] 1 round
   *[other] { $count } rounds
}
replays-incomplete = incomplet
patches-updating = Mise à jour…
patches-update-failed = Échec de la mise à jour: { $error }
patches-select-prompt = Sélectionnez un patch.
patches-readme-placeholder = Ce patch n'a pas de README.
patches-netplay-compatibility = Compatibilité netplay :
settings-section-general = Général
settings-section-graphics = Graphismes
settings-section-netplay = Netplay
settings-section-audio = Audio
settings-volume = Volume
settings-disable-bgm-in-pvp = Désactiver la musique pendant le netplay
settings-section-about = À propos
settings-section-input = Entrée
settings-input-press-key = Appuyez sur une touche ou un bouton…
settings-input-add = Ajouter une touche
settings-input-reset = Restaurer les valeurs par défaut
input-key-up = Haut
input-key-down = Bas
input-key-left = Gauche
input-key-right = Droite
input-key-a = A
input-key-b = B
input-key-l = L
input-key-r = R
input-key-start = Start
input-key-select = Select
input-key-speed-up = Avance rapide
input-gamepad-south = Bouton A
input-gamepad-east = Bouton B
input-gamepad-west = Bouton X
input-gamepad-north = Bouton Y
input-gamepad-select = Select
input-gamepad-start = Start
input-gamepad-mode = Guide
input-gamepad-left-thumb = Stick gauche
input-gamepad-right-thumb = Stick droit
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = D-Pad haut
input-gamepad-dpad-down = D-Pad bas
input-gamepad-dpad-left = D-Pad gauche
input-gamepad-dpad-right = D-Pad droite
input-gamepad-misc1 = Divers 1
input-gamepad-misc2 = Divers 2
input-gamepad-misc3 = Divers 3
input-gamepad-misc4 = Divers 4
input-gamepad-misc5 = Divers 5
input-gamepad-misc6 = Divers 6
input-gamepad-right-paddle1 = Palette droite 1
input-gamepad-left-paddle1 = Palette gauche 1
input-gamepad-right-paddle2 = Palette droite 2
input-gamepad-left-paddle2 = Palette gauche 2
input-gamepad-touchpad = Pavé tactile
input-gamepad-axis-left-stick-x = Stick gauche X
input-gamepad-axis-left-stick-y = Stick gauche Y
input-gamepad-axis-right-stick-x = Stick droit X
input-gamepad-axis-right-stick-y = Stick droit Y
input-gamepad-axis-trigger-left = Gâchette gauche
input-gamepad-axis-trigger-right = Gâchette droite
settings-enable-updater = Vérifier automatiquement les mises à jour
settings-allow-prerelease-upgrades = Inclure les préversions lors de la vérification
updater-current-version = Version actuelle: { $version }
updater-latest-version = Dernière version: { $version }
updater-loading = vérification…
updater-up-to-date = v{ $version } (à jour)
updater-downloading = Téléchargement: { $pct }%
updater-ready-to-update = Mise à jour téléchargée, prête à installer.
updater-update-now = Mettre à jour
welcome-title = Bienvenue dans Tango !
welcome-subtitle = Encore quelques étapes avant de pouvoir jouer.
welcome-step-roms = Ajoutez vos ROMs
welcome-step-roms-description = Placez vos fichiers .gba de Battle Network / Rockman EXE dans :
welcome-step-roms-detected = { $count } ROMs détectées.
welcome-step-nickname = Choisissez votre pseudo
welcome-step-nickname-description = Vous pourrez le changer à tout moment dans les Paramètres.
welcome-roms-needed = Ajoutez au moins une ROM pour continuer.
rescan = Réanalyser

# Reconnect / data folder / save-view tabs
session-stat-lead = Avance
save-tab-navicust = NaviCust
navi-edit-select = Navi
playback-reconnecting = Connexion perdue
playback-reconnecting-detail = Reconnexion…
settings-data-folder = Dossier des données
settings-data-folder-change = Changer…
