play-play = Jogar
save-tab-cover = Capa
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-file = Arquivo { $num }
auto-battle-data-secondary-standard-chips = Standard chips (secundários)
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
copied = Copiado!
save-copy-image = Copiar como imagem
navi-base-hp = HP
navi-buster-attack = Ataque
navi-buster-rapid = Rapidez
navi-buster-charge = Carga
navicust-grid-size = Grade: { $cols } × { $rows }
save-edit = Editar
save-edit-save = Salvar
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
build-chip-unknown = Chip nº { $id }
build-patch-card-unknown = Carta de patch nº { $id }
build-navicust-part-unknown = Peça do NaviCust nº { $id }
build-violation-navicust-materialization = A grade materializada do NaviCust não corresponde às peças instaladas.
build-violation-chip-illegal-for-program-deck = Não é um chip de programa válido para este slot.
build-violation-program-deck-exceeds-memory = O deck conectado usa { $used }MB; sua capacidade é { $limit }MB.
build-violation-slot-in-chip-exceeds-memory = Este chip SLOT IN usa { $used }MB; o limite é { $limit }MB.
build-violation-program-deck-missing-navi = O deck de programa não tem um chip Navi válido.
build-violation = { $subject }: { $reason }
build-violation-patch-cards-exceed-memory = Memória total dos Patch Cards: { $used }MB; o limite é { $limit }MB.
build-violation-patch-card4-wrong-slot-reason = Instalada no slot de Mod Card { $actual_slot }; pertence ao { $expected_slot }.
build-violation-patch-card4-not-in-catalog-reason = O slot de Mod Card { $actual_slot } não está no catálogo deste jogo.
build-violation-folder-not-full = { $required ->
    [one] O Folder contém { $used } do único chip necessário.
   *[other] O Folder contém { $used } dos { $required } chips necessários.
}
build-violation-chip-illegal-for-game = Não é permitido neste jogo ou versão.
build-violation-too-many-copies-of-chip = { $used ->
    [one] Há 1 cópia instalada; o limite é { $limit }.
   *[other] Há { $used } cópias instaladas; o limite é { $limit }.
}
build-violation-too-many-navi-chips = { $used ->
    [one] O Folder contém 1 chip Navi; o limite é { $limit }.
   *[other] O Folder contém { $used } chips Navi; o limite é { $limit }.
}
build-violation-too-many-mega-chips = { $used ->
    [one] O Folder contém 1 chip Mega; o limite é { $limit }.
   *[other] O Folder contém { $used } chips Mega; o limite é { $limit }.
}
build-violation-too-many-giga-chips = { $used ->
    [one] O Folder contém 1 chip Giga; o limite é { $limit }.
   *[other] O Folder contém { $used } chips Giga; o limite é { $limit }.
}
build-violation-too-many-dark-chips = { $used ->
    [one] O Folder contém 1 chip Dark; o limite é { $limit }.
   *[other] O Folder contém { $used } chips Dark; o limite é { $limit }.
}
build-violation-regular-chip-exceeds-memory = O chip Reg usa { $used }MB; o limite é { $limit }MB.
build-violation-tag-chips-exceed-memory = Os chips Tag usam { $used }MB; o limite é { $limit }MB.
build-violation-navicust-invalid-shape-reason = Colocada na grade com uma forma inválida.
build-violation-patch-card-exceeds-memory-with-contribution = Este Patch Card usa { $mb }MB; o total é { $used }MB; o limite é { $limit }MB.
folder-cannot-add-full = Não é possível adicionar: o Folder está cheio.
save-edit-sort = Ordenar
save-edit-clear = Limpar tudo
folder-sort-id = ID
folder-sort-name = Nome
folder-sort-code = Código
folder-sort-attack = Ataque
folder-sort-ap = AP
folder-sort-element = Elemento
folder-sort-mb = MB
folder-sort-hp = HP
navicust-edit-grid = NaviCust
navicust-edit-count = { $count ->
    [one] { $count } peça
   *[other] { $count } peças
}
navicust-edit-rotate = Girar
navicust-edit-compress = Comprimir
navicust-edit-uncompress = Descomprimir
navicust-edit-search = Buscar peças…
navicust-sort-id = ID
navicust-sort-name = Nome
navicust-sort-color = Cor
patch-card-edit-search = Buscar cartas…
patch-card-edit-count = { $count ->
    [one] { $count } carta
   *[other] { $count } cartas
}
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = Nome
patch-card-sort-mb = MB
patch-card4-none = Nenhuma
save-empty = Este save não tem dados para esta visualização.
save-tab-navicust = NaviCust
save-tab-program-deck = Program Deck
save-tab-party = Equipe
deck-mb = { $used }/{ $capacity }MB
deck-mb-uncapped = { $used }MB
deck-slot-in = Slot-in { $max }MB
bn5ds-leader = Líder
bn5ds-team-none = (nenhum)
bn5ds-chip-attack = Chip
bn5ds-partycust-add = Adicionar programa
bn5ds-partycust-empty = Sem programas
build-violation-partycust-gauge = { $used ->
    [one] O medidor do Customizador usa 1 bloco; o limite é { $limit }.
   *[other] O medidor do Customizador usa { $used } blocos; o limite é { $limit }.
}
build-violation-partycust-gauge-with-program = { $cost ->
    [one] Este programa usa 1 bloco; o total é { $used }; o limite é { $limit }.
   *[other] Este programa usa { $cost } blocos; o total é { $used }; o limite é { $limit }.
}
build-violation-partycust-copies = { $used ->
    [one] 1 cópia equipada; o limite é { $limit }.
   *[other] { $used } cópias equipadas; o limite é { $limit }.
}
navi-edit-select = Navi
