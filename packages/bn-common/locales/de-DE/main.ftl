play-play = Spielen
save-tab-cover = Deckel
save-review = Ansehen
save-tab-folder = Ordner
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-file = Datei { $num }
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
folder-group = Nach Chip gruppieren
save-copy = Kopieren
copied = Kopiert!
save-copy-image = Als Bild kopieren
navi-base-hp = HP
navi-buster-attack = Angriff
navi-buster-rapid = Rapid
navi-buster-charge = Charge
navicust-grid-size = Raster: { $cols } × { $rows }
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
build-chip-unknown = Chip Nr. { $id }
build-patch-card-unknown = Patch-Karte Nr. { $id }
build-navicust-part-unknown = NaviCust-Teil Nr. { $id }
build-violation-navicust-materialization = Das materialisierte NaviCust-Raster stimmt nicht mit den installierten Teilen überein.
build-violation-chip-illegal-for-program-deck = Kein zulässiger Programm-Chip für diesen Deckplatz.
build-violation-program-deck-exceeds-memory = Das verkabelte Deck nutzt { $used }MB; seine Kapazität beträgt { $limit }MB.
build-violation-slot-in-chip-exceeds-memory = Dieser SLOT-IN-Chip nutzt { $used }MB; das Limit beträgt { $limit }MB.
build-violation-program-deck-missing-navi = Im Programmdeck fehlt ein zulässiger Navi-Chip.
build-violation = { $subject }: { $reason }
build-violation-patch-cards-exceed-memory = Gesamter Patch-Card-Speicher: { $used }MB; das Limit beträgt { $limit }MB.
build-violation-patch-card4-wrong-slot-reason = An Mod-Card-Platz { $actual_slot } installiert; gehört aber auf { $expected_slot }.
build-violation-patch-card4-not-in-catalog-reason = Mod-Card-Platz { $actual_slot } ist nicht im Katalog dieses Spiels enthalten.
build-violation-folder-not-full = { $required ->
    [one] Der Ordner enthält { $used } von 1 erforderlichen Chip.
   *[other] Der Ordner enthält { $used } der erforderlichen { $required } Chips.
}
build-violation-chip-illegal-for-game = In diesem Spiel oder dieser Version nicht erlaubt.
build-violation-chip-code-unavailable = Dieser Code ist für diesen Chip nicht verfügbar.
build-violation-too-many-copies-of-chip = { $used ->
    [one] 1 Kopie ist installiert; das Limit beträgt { $limit }.
   *[other] { $used } Kopien sind installiert; das Limit beträgt { $limit }.
}
build-violation-too-many-navi-chips = { $used ->
    [one] Der Ordner enthält 1 Navi-Chip; das Limit beträgt { $limit }.
   *[other] Der Ordner enthält { $used } Navi-Chips; das Limit beträgt { $limit }.
}
build-violation-too-many-mega-chips = { $used ->
    [one] Der Ordner enthält 1 Mega-Chip; das Limit beträgt { $limit }.
   *[other] Der Ordner enthält { $used } Mega-Chips; das Limit beträgt { $limit }.
}
build-violation-too-many-giga-chips = { $used ->
    [one] Der Ordner enthält 1 Giga-Chip; das Limit beträgt { $limit }.
   *[other] Der Ordner enthält { $used } Giga-Chips; das Limit beträgt { $limit }.
}
build-violation-too-many-dark-chips = { $used ->
    [one] Der Ordner enthält 1 Dark-Chip; das Limit beträgt { $limit }.
   *[other] Der Ordner enthält { $used } Dark-Chips; das Limit beträgt { $limit }.
}
build-violation-regular-chip-exceeds-memory = Der Reg-Chip belegt { $used }MB; das Limit beträgt { $limit }MB.
build-violation-tag-chips-exceed-memory = Die Tag-Chips belegen { $used }MB; das Limit beträgt { $limit }MB.
build-violation-navicust-invalid-shape-reason = Mit einer ungültigen Form im Raster platziert.
build-violation-patch-card-exceeds-memory-with-contribution = Diese Patch Card belegt { $mb }MB; insgesamt sind es { $used }MB; das Limit beträgt { $limit }MB.
folder-cannot-add-full = Hinzufügen nicht möglich: Der Ordner ist voll.
save-edit-sort = Sortieren
save-edit-clear = Alle löschen
folder-sort-id = ID
folder-sort-name = Name
folder-sort-code = Code
folder-sort-attack = Angriff
folder-sort-ap = AP
folder-sort-element = Element
folder-sort-mb = MB
folder-sort-hp = HP
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
save-empty = Dieser Speicherstand hat keine Daten für diese Ansicht.
save-tab-navicust = NaviCust
save-tab-program-deck = Program Deck
save-tab-party = Party
deck-mb = { $used }/{ $capacity }MB
deck-mb-uncapped = { $used }MB
deck-slot-in = Slot-in { $max }MB
bn5ds-leader = Anführer
bn5ds-team-none = (keiner)
bn5ds-chip-attack = Chip
bn5ds-partycust-add = Programm hinzufügen
bn5ds-partycust-empty = Keine Programme
build-violation-partycust-gauge = { $used ->
    [one] Die Customizer-Leiste belegt 1 Block; das Limit beträgt { $limit }.
   *[other] Die Customizer-Leiste belegt { $used } Blöcke; das Limit beträgt { $limit }.
}
build-violation-partycust-gauge-with-program = { $cost ->
    [one] Dieses Programm belegt 1 Block; insgesamt sind es { $used }; das Limit beträgt { $limit }.
   *[other] Dieses Programm belegt { $cost } Blöcke; insgesamt sind es { $used }; das Limit beträgt { $limit }.
}
build-violation-partycust-copies = { $used ->
    [one] 1 Kopie ausgerüstet; das Limit beträgt { $limit }.
   *[other] { $used } Kopien ausgerüstet; das Limit beträgt { $limit }.
}
navi-edit-select = Navi
