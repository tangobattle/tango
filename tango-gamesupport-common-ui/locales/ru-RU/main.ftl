play-play = Воспроизвести
save-tab-cover = Покрытие
save-tab-folder = Папка
save-tab-patch-cards = Мод карты
save-tab-auto-battle-data = Данные автобоя
save-file = Файл { $num }
auto-battle-data-secondary-standard-chips = Стандартные чипы (второстепенные)
auto-battle-data-standard-chips = Стандартные чипы
auto-battle-data-mega-chips = Мега чипы
auto-battle-data-giga-chip = Гига чип
auto-battle-data-combos = Комбо
auto-battle-data-program-advance = Продвинутые программы
auto-battle-data-edit-used = Использовано
auto-battle-data-edit-secondary = Втор.
auto-battle-data-edit-count = { $count ->
    [one] { $count } чип
    [few] { $count } чипа
   *[other] { $count } чипов
}
folder-group = Группировать по чипу
save-copy = Копировать
copied = Скопировано!
save-copy-image = Копировать как изображение
navi-base-hp = HP
navi-buster-attack = Атака
navi-buster-rapid = Скорострельность
navi-buster-charge = Заряд
navicust-grid-size = Сетка: { $cols } × { $rows }
save-edit = Изменить
save-edit-save = Сохранить
save-edit-cancel = Отмена
folder-edit-search = Поиск чипов…
folder-edit-folder = Папка
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Нави { $used } / { $limit }
folder-edit-mega = Мега { $used } / { $limit }
folder-edit-giga = Гига { $used } / { $limit }
folder-edit-dark = Дарк { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
build-chip-unknown = Чип №{ $id }
build-patch-card-unknown = Мод-карта №{ $id }
build-navicust-part-unknown = Деталь NaviCust №{ $id }
build-violation-navicust-materialization = Материализованная сетка NaviCust не соответствует установленным деталям.
build-violation-chip-illegal-for-program-deck = Недопустимый программный чип для этого слота колоды.
build-violation-program-deck-exceeds-memory = Проводная колода использует { $used }MB при ёмкости { $limit }MB.
build-violation-slot-in-chip-exceeds-memory = Этот чип SLOT IN использует { $used }MB; предел — { $limit }MB.
build-violation-program-deck-missing-navi = В программной колоде нет допустимого чипа Нави.
build-violation = { $subject }: { $reason }
build-violation-patch-cards-exceed-memory = Общая память мод-карт: { $used } МБ; предел — { $limit } МБ.
build-violation-patch-card4-wrong-slot-reason = Установлена в слот мод-карты { $actual_slot }; должна находиться в { $expected_slot }.
build-violation-patch-card4-not-in-catalog-reason = Слот мод-карты { $actual_slot } отсутствует в каталоге этой игры.
build-violation-folder-not-full = { $required ->
    [one] В папке { $used } из { $required } необходимого чипа.
   *[other] В папке { $used } из { $required } необходимых чипов.
}
build-violation-chip-illegal-for-game = Недопустимо в этой игре или версии.
build-violation-chip-code-unavailable = Этот код недоступен для этого чипа.
build-violation-too-many-copies-of-chip = { $used ->
    [one] Установлена { $used } копия; лимит — { $limit }.
    [few] Установлены { $used } копии; лимит — { $limit }.
   *[other] Установлено { $used } копий; лимит — { $limit }.
}
build-violation-too-many-navi-chips = { $used ->
    [one] В папке { $used } Нави-чип; лимит — { $limit }.
    [few] В папке { $used } Нави-чипа; лимит — { $limit }.
   *[other] В папке { $used } Нави-чипов; лимит — { $limit }.
}
build-violation-too-many-mega-chips = { $used ->
    [one] В папке { $used } Мега-чип; лимит — { $limit }.
    [few] В папке { $used } Мега-чипа; лимит — { $limit }.
   *[other] В папке { $used } Мега-чипов; лимит — { $limit }.
}
build-violation-too-many-giga-chips = { $used ->
    [one] В папке { $used } Гига-чип; лимит — { $limit }.
    [few] В папке { $used } Гига-чипа; лимит — { $limit }.
   *[other] В папке { $used } Гига-чипов; лимит — { $limit }.
}
build-violation-too-many-dark-chips = { $used ->
    [one] В папке { $used } Дарк-чип; лимит — { $limit }.
    [few] В папке { $used } Дарк-чипа; лимит — { $limit }.
   *[other] В папке { $used } Дарк-чипов; лимит — { $limit }.
}
build-violation-regular-chip-exceeds-memory = Reg-чип занимает { $used }MB; лимит — { $limit }MB.
build-violation-tag-chips-exceed-memory = Tag-чипы занимают { $used }MB; лимит — { $limit }MB.
build-violation-navicust-invalid-shape-reason = Размещено в сетке с недопустимой формой.
build-violation-patch-card-exceeds-memory-with-contribution = Эта мод-карта занимает { $mb }MB; всего занято { $used }MB; лимит — { $limit }MB.
folder-cannot-add-full = Нельзя добавить: папка заполнена.
save-edit-sort = Сортировка
save-edit-clear = Очистить всё
folder-sort-id = ID
folder-sort-name = Название
folder-sort-code = Код
folder-sort-attack = Атака
folder-sort-ap = AP
folder-sort-element = Элемент
folder-sort-mb = MB
folder-sort-hp = HP
navicust-edit-grid = NaviCust
navicust-edit-count = { $count ->
    [one] { $count } деталь
    [few] { $count } детали
   *[other] { $count } деталей
}
navicust-edit-rotate = Повернуть
navicust-edit-compress = Сжать
navicust-edit-uncompress = Разжать
navicust-edit-search = Поиск деталей…
navicust-sort-id = ID
navicust-sort-name = Название
navicust-sort-color = Цвет
patch-card-edit-search = Поиск карт…
patch-card-edit-count = { $count ->
    [one] { $count } карта
    [few] { $count } карты
   *[other] { $count } карт
}
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = Название
patch-card-sort-mb = MB
patch-card4-none = Нет
save-empty = В этом сохранении нет данных для этого вида.
save-tab-navicust = NaviCust
save-tab-program-deck = Колода программ
save-tab-party = Отряд
deck-mb = { $used }/{ $capacity }MB
deck-mb-uncapped = { $used }MB
deck-slot-in = Слот-ин { $max }MB
bn5ds-leader = Лидер
bn5ds-team-none = (нет)
bn5ds-chip-attack = Чип
bn5ds-partycust-add = Добавить программу
bn5ds-partycust-empty = Нет программ
build-violation-partycust-gauge = { $used ->
    [one] Шкала настройщика использует { $used } блок; предел — { $limit }.
    [few] Шкала настройщика использует { $used } блока; предел — { $limit }.
   *[other] Шкала настройщика использует { $used } блоков; предел — { $limit }.
}
build-violation-partycust-gauge-with-program = { $cost ->
    [one] Эта программа занимает { $cost } блок; всего занято { $used }; предел — { $limit }.
    [few] Эта программа занимает { $cost } блока; всего занято { $used }; предел — { $limit }.
   *[other] Эта программа занимает { $cost } блоков; всего занято { $used }; предел — { $limit }.
}
build-violation-partycust-copies = { $used ->
    [one] Установлена { $used } копия; предел — { $limit }.
    [few] Установлены { $used } копии; предел — { $limit }.
   *[other] Установлено { $used } копий; предел — { $limit }.
}
navi-edit-select = Нави
