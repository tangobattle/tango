## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = Русский
window-quit = Выйти из Танго
tab-play = Игра
tab-replays = Повторы
tab-patches = Патчи
tab-settings = Настройки
play-no-game = No game selected
play-no-save = Выбрать сохранение
play-no-patch = No patch
play-version-placeholder = —
play-link-code = Код ссылки (оставьте пустым для случайного)
play-play = Воспроизвести
play-fight = В бой!
play-cancel = Отмена
play-no-selection = Выберите игру и сохранение для просмотра.
play-status-connecting = Подключение к серверу подбора…
play-status-waiting-opponent = Ожидание соперника…
play-you = Вы
play-opponent = Соперник
empty-no-roms-title = ROM-файлы не найдены
empty-no-roms-body = Поместите ваши файлы .gba от Battle Network / Rockman EXE в:
lobby-waiting = Ожидание…
lobby-latency = Пинг: { $ms } мс
lobby-link-code = Код ссылки: { $code }
lobby-match-type = Тип матча
lobby-ready = Готов
lobby-unready = Не готов
lobby-match-starting = Запуск…
lobby-compat-ok = Совместимо — готов к игре.
lobby-compat-missing-game = Одна сторона не выбрала игру.
lobby-compat-missing-rom = Игра или патч не установлены с обеих сторон.
lobby-compat-match-mismatch = Тип матча не совпадает.
lobby-pick-game-first = Сначала выберите игру
lobby-no-match-types = (нет типов матча для этой игры)
session-results-victory = Победа!
session-results-defeat = Поражение
session-results-draw = Ничья
session-results-no-contest = Матч завершён
session-results-vs = против { $nickname }
session-results-you = Вы
session-results-round = Раунд { $number }
session-results-draws = { $count ->
    [one] { $count } раунд завершился вничью
    [few] { $count } раунда завершились вничью
   *[many] { $count } раундов завершились вничью
}
session-results-done = Готово
discord-presence-in-single-player = В одиночной игре
discord-presence-in-progress = Матч в процессе
playback-pause = Пауза
playback-close = Закрыть
replays-watch = Смотреть
replays-incomplete = неполный
replays-watch-missing-rom = Смотреть (ROM этой игры не найден)
save-delete = Удалить
patches-update = Обновить
patches-updating = Обновление…
patches-update-failed = Сбой обновления: { $error }
patches-netplay-compatibility = Совместимость netplay:
patches-details-games = Поддерживаемые игры:
patches-select-prompt = Выберите патч.
settings-patch-repo = Репозитория Патчей
settings-section-general = Общие
settings-section-graphics = Графика
settings-section-audio = Звук
settings-section-input = Управление
settings-section-about = О программе
settings-volume = Громкость
settings-nickname = Ник
settings-language = Язык
settings-input-press-key = Нажмите клавишу или кнопку…
settings-input-reset = Сбросить по умолчанию
settings-input-select-hint = Нажмите на кнопку, чтобы изменить её привязки
welcome-title = Добро пожаловать в Tango!
welcome-subtitle = Осталось пару шагов до начала игры.
welcome-continue = Я Готов!
welcome-step-roms = Добавьте свои ROM
welcome-step-roms-detected = Обнаружено { $count } ROM.
welcome-step-nickname = Задайте свой ник
welcome-roms-needed = Добавьте хотя бы один ROM, чтобы продолжить.

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = NaviCust
save-tab-folder = Папка
save-tab-patch-cards = Мод карты
save-tab-auto-battle-data = Данные автобоя
save-empty = В этом сохранении нет данных для этого вида.
save-copy = Копировать
save-copy-image = Копировать как изображение
copied = Скопировано!
save-edit = Изменить
save-edit-save = Сохранить
save-edit-cancel = Отмена
save-edit-sort = Сортировка
save-edit-clear = Очистить всё
folder-group = Группировать по чипу
navi-style = Стиль
navi-style-unset = (без стиля)
navi-id = ID Navi
navi-link-navi = Link Navi
navi-base-hp = HP
navi-buster = Бастер
navi-buster-attack = Атака
navi-buster-rapid = Скорострельность
navi-buster-charge = Заряд
navi-power-attack = Мощная атака
navi-edit-select = Нави
folder-edit-search = Поиск чипов…
folder-edit-folder = Папка
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Нави { $used } / { $limit }
folder-edit-mega = Мега { $used } / { $limit }
folder-edit-giga = Гига { $used } / { $limit }
folder-edit-dark = Дарк { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
folder-sort-id = ID
folder-sort-name = Название
folder-sort-code = Код
folder-sort-attack = Атака
folder-sort-element = Элемент
folder-sort-mb = MB
navicust-grid-size = Сетка: { $cols } × { $rows }
navicust-parts = Установленные детали
navicust-empty = (ничего не установлено)
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
save-actions = Действия с сохранением
save-duplicate = Дублировать
save-rename = Переименовать
save-rename-confirm = Переименовать
save-delete-confirm = Удалить
save-action-cancel = Отмена
save-delete-prompt = Удалить { $name }?
save-name-placeholder = Новое имя
save-new = Новое сохранение
save-new-confirm = Создать
save-template-default = (по умолчанию)
save-template-pick = Выбрать шаблон…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = Все игры
replays-filter-any-time = За всё время
replays-filter-past-day = Последние 24 часа
replays-filter-past-week = За последнюю неделю
replays-filter-past-month = За последний месяц
replays-filter-past-year = За последний год
replays-filter-search-placeholder = Поиск повторов…
replays-show-incomplete = Показывать неполные
replays-direct-marker = (прямое)
replays-select-prompt = Выберите повтор.
replays-match-type = Тип матча:
replays-duration = Длительность:
replays-round-count = { $count ->
    [one] 1 раунд
    [few] { $count } раунда
   *[many] { $count } раундов
}

## patches detail (extracted from the desktop's main.ftl; keep in sync)
patches-favorite = В избранное
patches-unfavorite = Убрать из избранного
patches-search-placeholder = Поиск патчей…
patches-readme-placeholder = У этого патча нет README.
patches-details-authors = Авторы:
patches-details-license = Лицензия:
    .all-rights-reserved = Все права защищены
patches-details-source = Сайт:

## netplay settings (extracted from the desktop's main.ftl; keep in sync)
settings-matchmaking-endpoint = Точка окончания матча
settings-use-relay = Использовать сервер-ретранслятор
settings-use-relay-auto = Автоматически
settings-use-relay-always = Всегда
settings-use-relay-never = Никогда
settings-show-opponent-setup = Показывать сборку соперника в начале матча

## netplay settings section label (extracted from the desktop)
settings-section-netplay = Сетевая игра

## accent + patch repo settings (extracted from the desktop)
settings-accent = Акцентный цвет
settings-accent-tango-green = Зелёный Tango
settings-accent-megaman-blue = Синий MegaMan
settings-accent-protoman-red = Красный ProtoMan
settings-accent-roll-pink = Розовый Roll
settings-accent-gutsman-yellow = Жёлтый GutsMan
settings-accent-bass-purple = Фиолетовый Bass

## welcome step description (extracted from the desktop)
welcome-step-nickname-description = Можно изменить в любой момент в Настройках.

## replay video export (extracted from the desktop)
replays-export = Экспортировать
replays-export-progress = Рендеринг…
replays-export-cancel = Отмена
replays-export-success = Рендер завершён.
replays-export-error = Сбой рендера: { $error }

## theme + streamer + autoupdate settings (extracted from the desktop)
settings-theme = Тема оформления
settings-theme-dark = Тёмная
settings-theme-light = Светлая
settings-streamer-mode = Режим приватности стримера
settings-enable-patch-autoupdate = Включить авто обновление

## mute bgm setting (extracted from the desktop)
settings-disable-bgm-in-pvp = Отключить музыку в сетевой игре

## cover tab (extracted from the desktop)
save-tab-cover = Покрытие

## lobby + telemetry + transport (extracted from the desktop)
lobby-blind-mine = Скрыть сборку
lobby-blind-self-on = Вы скрываете свою сборку.
lobby-blind-peer-on = Соперник скрывает свою сборку.
settings-netplay-frame-delay = Задержка кадров
lobby-frame-delay-suggest = Предложить по пингу
session-stat-tps = Тик/с (текущее/макс.)
session-stat-skew = Смещение
session-stat-lead = Опережение
session-stat-depth = Глубина ошибки предсказания
session-stat-ping = Сетевая задержка
playback-speed = Скорость
playback-input-display = Отображение ввода

## replay input display + swap (extracted from the desktop)
playback-swap-perspective = Вид соперника

## pvp setup drawers (extracted from the desktop)
session-self = Моя сборка
session-opponent = Сборка соперника

## replay pip (extracted from the desktop's main.ftl; keep in sync)
playback-pip = Экран соперника

## replay transport (extracted from the desktop's main.ftl; keep in sync)
playback-play = Воспроизвести
playback-clip-tools = Клип
playback-clip-start = Отметить начало клипа
playback-clip-end = Отметить конец клипа
playback-clip-clear = Сбросить метки клипа
playback-clip-export = Экспортировать клип

## export cancelling (extracted from the desktop's main.ftl; keep in sync)
replays-export-cancelling = Отмена…

## video filter (extracted from the desktop's main.ftl; keep in sync)
settings-video-filter = Видео фильтр
    .null = Ничего
    .hq2x = hq2x
    .hq3x = hq3x
    .hq4x = hq4x
    .mmpx = MMPX
