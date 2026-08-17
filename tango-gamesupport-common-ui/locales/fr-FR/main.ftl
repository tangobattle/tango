play-play = Jouer
save-tab-cover = Couverture
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-file = Fichier { $num }
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
folder-group = Grouper par puce
save-copy = Copier
copied = Copié !
save-copy-image = Copier en image
navi-base-hp = HP
navi-buster-attack = Attaque
navi-buster-rapid = Cadence
navi-buster-charge = Charge
navicust-grid-size = Grille: { $cols } × { $rows }
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
build-chip-unknown = Puce nº { $id }
build-patch-card-unknown = Patch Card nº { $id }
build-navicust-part-unknown = Pièce NaviCust nº { $id }
build-violation-navicust-materialization = La grille NaviCust matérialisée ne correspond pas aux pièces installées.
build-violation-chip-illegal-for-program-deck = Cette puce programme n’est pas autorisée dans cet emplacement.
build-violation-program-deck-exceeds-memory = Le deck câblé utilise { $used }MB ; sa capacité est de { $limit }MB.
build-violation-slot-in-chip-exceeds-memory = Cette puce SLOT IN utilise { $used }MB ; la limite est de { $limit }MB.
build-violation-program-deck-missing-navi = Le deck programme ne contient aucune puce Navi valide.
build-violation = { $subject } : { $reason }
build-violation-patch-cards-exceed-memory = Mémoire totale des Patch Cards : { $used } Mo ; la limite est de { $limit } Mo.
build-violation-patch-card4-wrong-slot-reason = Installée dans l’emplacement Mod Card { $actual_slot } ; appartient à { $expected_slot }.
build-violation-patch-card4-not-in-catalog-reason = L’emplacement Mod Card { $actual_slot } ne figure pas au catalogue de ce jeu.
build-violation-folder-not-full = { $required ->
    [one] Le Folder contient { $used } de l’unique puce requise.
   *[other] Le Folder contient { $used } des { $required } puces requises.
}
build-violation-chip-illegal-for-game = Non autorisée dans ce jeu ou cette version.
build-violation-too-many-copies-of-chip = { $used ->
    [one] 1 exemplaire est installé ; la limite est de { $limit }.
   *[other] { $used } exemplaires sont installés ; la limite est de { $limit }.
}
build-violation-too-many-navi-chips = { $used ->
    [one] Le Folder contient 1 puce Navi ; la limite est de { $limit }.
   *[other] Le Folder contient { $used } puces Navi ; la limite est de { $limit }.
}
build-violation-too-many-mega-chips = { $used ->
    [one] Le Folder contient 1 puce Mega ; la limite est de { $limit }.
   *[other] Le Folder contient { $used } puces Mega ; la limite est de { $limit }.
}
build-violation-too-many-giga-chips = { $used ->
    [one] Le Folder contient 1 puce Giga ; la limite est de { $limit }.
   *[other] Le Folder contient { $used } puces Giga ; la limite est de { $limit }.
}
build-violation-too-many-dark-chips = { $used ->
    [one] Le Folder contient 1 puce Dark ; la limite est de { $limit }.
   *[other] Le Folder contient { $used } puces Dark ; la limite est de { $limit }.
}
build-violation-regular-chip-exceeds-memory = La puce Reg utilise { $used }MB ; la limite est de { $limit }MB.
build-violation-tag-chips-exceed-memory = Les puces Tag utilisent { $used }MB ; la limite est de { $limit }MB.
build-violation-navicust-invalid-shape-reason = Placée sur la grille avec une forme invalide.
build-violation-patch-card-exceeds-memory-with-contribution = Cette Patch Card utilise { $mb } Mo ; le total est de { $used } Mo ; la limite est de { $limit } Mo.
folder-cannot-add-full = Ajout impossible : le Folder est plein.
save-edit-sort = Trier
save-edit-clear = Tout effacer
folder-sort-id = ID
folder-sort-name = Nom
folder-sort-code = Code
folder-sort-attack = Attaque
folder-sort-ap = AP
folder-sort-element = Élément
folder-sort-mb = MB
folder-sort-hp = HP
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
save-empty = Cette sauvegarde n'a pas de données pour cette vue.
save-tab-navicust = NaviCust
save-tab-program-deck = Program Deck
save-tab-party = Équipe
deck-mb = { $used }/{ $capacity }MB
deck-mb-uncapped = { $used }MB
deck-slot-in = Slot-in { $max }MB
bn5ds-leader = Chef
bn5ds-team-none = (aucun)
bn5ds-chip-attack = Chip
bn5ds-partycust-add = Ajouter un programme
bn5ds-partycust-empty = Aucun programme
build-violation-partycust-gauge = { $used ->
    [one] La jauge du personnalisateur utilise 1 bloc ; la limite est de { $limit }.
   *[other] La jauge du personnalisateur utilise { $used } blocs ; la limite est de { $limit }.
}
build-violation-partycust-gauge-with-program = { $cost ->
    [one] Ce programme utilise 1 bloc ; le total est de { $used } ; la limite est de { $limit }.
   *[other] Ce programme utilise { $cost } blocs ; le total est de { $used } ; la limite est de { $limit }.
}
build-violation-partycust-copies = { $used ->
    [one] 1 exemplaire équipé ; la limite est de { $limit }.
   *[other] { $used } exemplaires équipés ; la limite est de { $limit }.
}
navi-edit-select = Navi
