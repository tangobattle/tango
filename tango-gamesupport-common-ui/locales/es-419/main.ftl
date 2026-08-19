play-play = Jugar
save-tab-cover = Cubierta
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-file = Archivo { $num }
auto-battle-data-secondary-standard-chips = Standard chips (secundarios)
auto-battle-data-standard-chips = Standard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combos
auto-battle-data-program-advance = Program advance
auto-battle-data-edit-used = Usos
auto-battle-data-edit-secondary = Sec.
auto-battle-data-edit-count = { $count ->
    [one] { $count } chip
   *[other] { $count } chips
}
folder-group = Agrupar por chip
save-copy = Copiar
copied = ¡Copiado!
save-copy-image = Copiar como imagen
navi-base-hp = HP
navi-buster-attack = Ataque
navi-buster-rapid = Rapidez
navi-buster-charge = Carga
navicust-grid-size = Cuadrícula: { $cols } × { $rows }
save-edit = Editar
save-edit-save = Guardar
save-edit-cancel = Cancelar
folder-edit-search = Buscar chips…
folder-edit-folder = Folder
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
build-chip-unknown = Chip n.º { $id }
build-patch-card-unknown = Tarjeta de parche n.º { $id }
build-navicust-part-unknown = Pieza de NaviCust n.º { $id }
build-violation-navicust-materialization = La cuadrícula materializada de NaviCust no coincide con las piezas instaladas.
build-violation-chip-illegal-for-program-deck = No es un chip de programa válido para esta ranura.
build-violation-program-deck-exceeds-memory = El deck conectado usa { $used }MB; su capacidad es de { $limit }MB.
build-violation-slot-in-chip-exceeds-memory = Este chip SLOT IN usa { $used }MB; el límite es de { $limit }MB.
build-violation-program-deck-missing-navi = El deck de programa no tiene un chip Navi válido.
build-violation = { $subject }: { $reason }
build-violation-patch-cards-exceed-memory = Memoria total de Patch Cards: { $used }MB; el límite es { $limit }MB.
build-violation-patch-card4-wrong-slot-reason = Instalada en la ranura Mod Card { $actual_slot }; pertenece a { $expected_slot }.
build-violation-patch-card4-not-in-catalog-reason = La ranura Mod Card { $actual_slot } no está en el catálogo de este juego.
build-violation-folder-not-full = { $required ->
    [one] El Folder contiene { $used } del único chip requerido.
   *[other] El Folder contiene { $used } de los { $required } chips requeridos.
}
build-violation-chip-illegal-for-game = No es válido en este juego o versión.
build-violation-chip-code-unavailable = Este código no está disponible para este chip.
build-violation-too-many-copies-of-chip = { $used ->
    [one] Hay 1 copia instalada; el límite es { $limit }.
   *[other] Hay { $used } copias instaladas; el límite es { $limit }.
}
build-violation-too-many-navi-chips = { $used ->
    [one] El Folder contiene 1 chip Navi; el límite es { $limit }.
   *[other] El Folder contiene { $used } chips Navi; el límite es { $limit }.
}
build-violation-too-many-mega-chips = { $used ->
    [one] El Folder contiene 1 chip Mega; el límite es { $limit }.
   *[other] El Folder contiene { $used } chips Mega; el límite es { $limit }.
}
build-violation-too-many-giga-chips = { $used ->
    [one] El Folder contiene 1 chip Giga; el límite es { $limit }.
   *[other] El Folder contiene { $used } chips Giga; el límite es { $limit }.
}
build-violation-too-many-dark-chips = { $used ->
    [one] El Folder contiene 1 chip Dark; el límite es { $limit }.
   *[other] El Folder contiene { $used } chips Dark; el límite es { $limit }.
}
build-violation-regular-chip-exceeds-memory = El chip Reg usa { $used }MB; el límite es { $limit }MB.
build-violation-tag-chips-exceed-memory = Los chips Tag usan { $used }MB; el límite es { $limit }MB.
build-violation-navicust-invalid-shape-reason = Colocada en la cuadrícula con una forma no válida.
build-violation-patch-card-exceeds-memory-with-contribution = Esta Patch Card usa { $mb }MB; el total es { $used }MB; el límite es { $limit }MB.
folder-cannot-add-full = No se puede añadir: el Folder está lleno.
save-edit-sort = Ordenar
save-edit-clear = Borrar todo
folder-sort-id = ID
folder-sort-name = Nombre
folder-sort-code = Código
folder-sort-attack = Ataque
folder-sort-ap = AP
folder-sort-element = Elemento
folder-sort-mb = MB
folder-sort-hp = HP
navicust-edit-grid = NaviCust
navicust-edit-count = { $count ->
    [one] { $count } pieza
   *[other] { $count } piezas
}
navicust-edit-rotate = Rotar
navicust-edit-compress = Comprimir
navicust-edit-uncompress = Descomprimir
navicust-edit-search = Buscar piezas…
navicust-sort-id = ID
navicust-sort-name = Nombre
navicust-sort-color = Color
patch-card-edit-search = Buscar tarjetas…
patch-card-edit-count = { $count ->
    [one] { $count } tarjeta
   *[other] { $count } tarjetas
}
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = Nombre
patch-card-sort-mb = MB
patch-card4-none = Ninguna
save-empty = Este guardado no tiene datos para esta vista.
save-tab-navicust = NaviCust
save-tab-program-deck = Program Deck
save-tab-party = Equipo
deck-mb = { $used }/{ $capacity }MB
deck-mb-uncapped = { $used }MB
deck-slot-in = Slot-in { $max }MB
bn5ds-leader = Líder
bn5ds-team-none = (ninguno)
bn5ds-chip-attack = Chip
bn5ds-partycust-add = Añadir programa
bn5ds-partycust-empty = Sin programas
build-violation-partycust-gauge = { $used ->
    [one] El medidor del personalizador usa 1 bloque; el límite es { $limit }.
   *[other] El medidor del personalizador usa { $used } bloques; el límite es { $limit }.
}
build-violation-partycust-gauge-with-program = { $cost ->
    [one] Este programa usa 1 bloque; el total es { $used }; el límite es { $limit }.
   *[other] Este programa usa { $cost } bloques; el total es { $used }; el límite es { $limit }.
}
build-violation-partycust-copies = { $used ->
    [one] 1 copia equipada; el límite es { $limit }.
   *[other] { $used } copias equipadas; el límite es { $limit }.
}
navi-edit-select = Navi
