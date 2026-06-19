# Endonym for this locale; shown in the language picker.
LANGUAGE = Русский

crash =
    Ой, Танго столкнулся с ошибкой и Сломался!

    При сообщении о Поломке, пожалуйста, укажите следующий файл журнала:

    { $path }
crash-no-log =
    Упс, Танго столкнулся с ошибкой и Сломался!

    { $error }
window-title = Танго
    .running = Танго(запушен)
# Tooltip on the top bar's close button (fullscreen only).
window-quit = Выйти из Танго
play-play = Воспроизвести
play-no-game = No game selected
play-no-patch = No patch
play-patch-toggle = Использовать патч…
play-you = Вы
play-cancel = Отмена
replays-export = Экспортировать
replays-export-disable-bgm = Отключить музыку
replays-export-twosided = Двусторонний
replays-export-success = Рендер завершён.
replays-export-error = Сбой рендера: { $error }
replays-export-open = Open
patches-open-folder = Открыть папку
patches-favorite = В избранное
patches-unfavorite = Убрать из избранного
patches-search-placeholder = Поиск патчей…
patches-update = Обновить
patches-details-authors = Авторы:
patches-details-license = Лицензия:
    .all-rights-reserved = Все права защищены
patches-details-source = Сайт:
patches-details-games = Поддерживаемые игры:
settings-theme = Тема оформления
settings-theme-dark = Тёмная
settings-theme-light = Светлая
    .light = Светлая
    .dark = Темная
    .system = Следовать настройкам системы
settings-video-filter = Видео фильтр
    .null = Ничего
    .hq2x = hq2x
    .hq3x = hq3x
    .hq4x = hq4x
    .mmpx = MMPX
settings-language = Язык
settings-nickname = Ник
settings-streamer-mode = Режим приватности стримера
settings-section-experimental = Экспериментальное
settings-enable-save-editor = Включить редактор сохранений
settings-experimental-warning = Экспериментальные функции могут повредить ваши сохранения или сделать их непригодными, могут быть изменены или удалены в любой момент и могут не иметь проверок, обеспечивающих допустимость сохранений для онлайн-игры. Используйте на свой страх и риск.
    .tooltip = Включение этого режима добавит дополнительную вкладку "Обложение" в окно сохранения, которая скрывает всю информацию о вашем текущем файле сохранения.
settings-matchmaking-endpoint = Точка окончания матча
settings-patch-repo = Репозитория Патчей
settings-enable-patch-autoupdate = Включить авто обновление
settings-data-path = Путь к данным
    .open = Открыть
    .change = Изменить
settings-window-size = Размер окна
settings-fullscreen = Полноэкранный режим
settings-ui-scale = Масштаб интерфейса
settings-fractional-scaling = Дробное масштабирование
settings-hide-emulator-border = Скрыть рамку эмулятора
save-tab-cover = Покрытие
save-tab-navi = Нави
save-tab-folder = Папка
save-tab-patch-cards = Мод карты
save-tab-auto-battle-data = Данные автобоя
auto-battle-data-secondary-standard-chips = Стандартные чипы (второстепенные)
auto-battle-data-standard-chips = Стандартные чипы
auto-battle-data-mega-chips = Мега чипы
auto-battle-data-giga-chip = Гига чип
auto-battle-data-combos = Комбо
auto-battle-data-program-advance = Продвинутые программы

# Auto Battle Data editor
auto-battle-data-edit-used = Использовано
auto-battle-data-edit-secondary = Втор.
auto-battle-data-edit-count = { $count ->
    [one] { $count } чип
    [few] { $count } чипа
   *[other] { $count } чипов
}
welcome-open-folder = Открыть папку
welcome-continue = Я Готов!
discord-presence-looking = Поиск матча
discord-presence-in-single-player = В одиночной игре
discord-presence-in-lobby = В лобби
discord-presence-in-progress = Матч в процессе

# === translations ===
tab-play = Игра
tab-replays = Повторы
tab-patches = Патчи
tab-settings = Настройки
play-no-save = Выбрать сохранение
save-open-folder = Открыть папку
save-duplicate = Дублировать
save-rename = Переименовать
save-delete = Удалить
save-rename-confirm = Переименовать
save-delete-confirm = Удалить
save-action-cancel = Отмена
save-delete-prompt = Удалить { $name }?
save-name-placeholder = Новое имя
save-new = Новое сохранение
save-new-confirm = Создать
save-template-default = (по умолчанию)
save-template-pick = Выбрать шаблон…
empty-no-roms-title = ROM-файлы не найдены
empty-no-roms-body = Поместите ваши файлы .gba от Battle Network / Rockman EXE в:
empty-no-saves-title = Нет сохранений для этой игры
empty-no-saves-body = Поместите .sav для этой игры в:
play-version-placeholder = —
play-status-connecting = Подключение к серверу подбора…
play-status-direct-connecting = Подключение к сопернику…
play-status-waiting-opponent = Ожидание соперника…
play-status-failed = Сбой подключения: { $error }
play-status-peer-disconnected = Другой игрок покинул игру.
play-status-negotiate-expected-hello = Другой игрок не отправил ожидаемое рукопожатие.
play-status-negotiate-version-too-old = Другой игрок использует более старую версию Tango.
play-status-negotiate-version-too-new = Другой игрок использует более новую версию Tango.
play-status-negotiate-failed = Произошла ошибка во время согласования: { $error }
lobby-waiting = Ожидание…
lobby-no-game = (игра не выбрана)
lobby-latency = Пинг: { $ms } мс
lobby-latency-direct = Пинг (напрямую): { $ms } мс
lobby-latency-relayed = Пинг (через ретранслятор): { $ms } мс
lobby-link-code = Код ссылки: { $code }
lobby-direct-host = Хостинг на UDP-порту: { $port }
lobby-direct-connect = Подключение через UDP: { $target }
lobby-handshake = Обмен настройками…
lobby-match-type = Тип матча
settings-netplay-frame-delay = Задержка кадров
settings-use-relay = Использовать сервер-ретранслятор
settings-use-relay-auto = Автоматически
settings-use-relay-always = Всегда
settings-use-relay-never = Никогда
lobby-frame-delay-suggest = Предложить по пингу
lobby-no-match-types = (нет типов матча для этой игры)
lobby-pick-game-first = Сначала выберите игру
lobby-compat-ok = Совместимо — готов к игре.
lobby-compat-missing-game = Одна сторона не выбрала игру.
lobby-compat-missing-rom = Игра или патч не установлены с обеих сторон.
lobby-compat-version-mismatch = Версии игры не совпадают (разные патч / ROM).
lobby-compat-match-mismatch = Тип матча не совпадает.
lobby-ready = Готов
lobby-unready = Не готов
lobby-match-starting = Запуск…
lobby-blind-mine = Скрыть сборку
lobby-blind-peer-on = Соперник скрывает свою сборку.
lobby-blind-self-on = Вы скрываете свою сборку.
session-opponent = Сборка соперника
session-self = Моя сборка
session-back-to-session = Вернуться к сессии
# PvP telemetry deck cell tooltips
session-stat-tps = Тик/с (текущее/макс.)
session-stat-skew = Смещение
session-stat-depth = Глубина ошибки предсказания
session-stat-ping = Сетевая задержка
navi-style = Стиль
folder-group = Группировать по чипу
save-copy = Копировать
copied = Скопировано!
save-copy-image = Копировать как изображение
navi-id = ID Navi
navi-link-navi = Link Navi
navi-style-unset = (без стиля)
navicust-grid-size = Сетка: { $cols } × { $rows }
navicust-parts = Установленные детали
navicust-empty = (ничего не установлено)

# Folder editor
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
save-edit-sort = Сортировка
save-edit-clear = Очистить всё
folder-sort-id = ID
folder-sort-name = Название
folder-sort-code = Код
folder-sort-attack = Атака
folder-sort-element = Элемент
folder-sort-mb = MB

# Navicust editor
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

# Patch card editor
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
play-no-selection = Выберите игру и сохранение для просмотра.
replays-filter-all-games = Все игры
replays-filter-opponent-placeholder = Любой
replays-show-incomplete = Показывать неполные
replays-direct-marker = (прямое)
replays-watch = Смотреть
replays-watch-missing-rom = Смотреть (ROM этой игры не найден)
replays-export-progress = Рендеринг…
replays-export-cancel = Отмена
replays-export-cancelling = Отмена…
replays-export-reset = Сбросить
replays-export-scale = Масштаб
replays-export-scale-lossless = без потерь
replays-export-rounds = Раунды:
replays-export-save-as = Сохранить как…
playback-close = Закрыть
playback-options = Параметры
playback-speed = Скорость
playback-play = Воспроизвести
playback-pause = Пауза
playback-disconnect = Отключиться
playback-disconnect-prompt = Отключиться от этого матча?
playback-disconnect-detail = Вы завершите матч с соперником.
playback-cancel = Отмена
replays-select-prompt = Выберите повтор.
play-opponent = Соперник
replays-match-type = Тип матча:
replays-duration = Длительность:
replays-round-count = { $count ->
    [one] 1 раунд
    [few] { $count } раунда
   *[many] { $count } раундов
}
replays-incomplete = неполный
patches-updating = Обновление…
patches-update-failed = Сбой обновления: { $error }
patches-select-prompt = Выберите патч.
patches-readme-placeholder = У этого патча нет README.
patches-netplay-compatibility = Совместимость netplay:
settings-section-general = Общие
settings-section-graphics = Графика
settings-section-netplay = Сетевая игра
settings-section-audio = Звук
settings-volume = Громкость
settings-disable-bgm-in-pvp = Отключить музыку в сетевой игре
settings-section-about = О программе
settings-section-input = Управление
settings-input-press-key = Нажмите клавишу или кнопку…
settings-input-add = Добавить привязку
settings-input-reset = Сбросить по умолчанию
input-key-up = Вверх
input-key-down = Вниз
input-key-left = Влево
input-key-right = Вправо
input-key-a = A
input-key-b = B
input-key-l = L
input-key-r = R
input-key-start = Start
input-key-select = Select
input-key-speed-up = Ускорение
input-gamepad-south = Кнопка A
input-gamepad-east = Кнопка B
input-gamepad-west = Кнопка X
input-gamepad-north = Кнопка Y
input-gamepad-select = Select
input-gamepad-start = Start
input-gamepad-mode = Guide
input-gamepad-left-thumb = Левый стик
input-gamepad-right-thumb = Правый стик
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = Крестовина вверх
input-gamepad-dpad-down = Крестовина вниз
input-gamepad-dpad-left = Крестовина влево
input-gamepad-dpad-right = Крестовина вправо
input-gamepad-misc1 = Доп. 1
input-gamepad-misc2 = Доп. 2
input-gamepad-misc3 = Доп. 3
input-gamepad-misc4 = Доп. 4
input-gamepad-misc5 = Доп. 5
input-gamepad-misc6 = Доп. 6
input-gamepad-right-paddle1 = Правый лепесток 1
input-gamepad-left-paddle1 = Левый лепесток 1
input-gamepad-right-paddle2 = Правый лепесток 2
input-gamepad-left-paddle2 = Левый лепесток 2
input-gamepad-touchpad = Тачпад
input-gamepad-axis-left-stick-x = Левый стик X
input-gamepad-axis-left-stick-y = Левый стик Y
input-gamepad-axis-right-stick-x = Правый стик X
input-gamepad-axis-right-stick-y = Правый стик Y
input-gamepad-axis-trigger-left = Левый триггер
input-gamepad-axis-trigger-right = Правый триггер
settings-enable-updater = Автоматически проверять обновления
settings-allow-prerelease-upgrades = Учитывать предварительные версии при проверке
updater-current-version = Текущая версия: { $version }
updater-latest-version = Последняя версия: { $version }
updater-loading = проверка…
updater-up-to-date = v{ $version } (актуальная)
updater-downloading = Загрузка: { $pct }%
updater-ready-to-update = Обновление загружено и готово к установке.
updater-update-now = Обновить сейчас
welcome-title = Добро пожаловать в Tango!
welcome-subtitle = Осталось пару шагов до начала игры.
welcome-step-roms = Добавьте свои ROM
welcome-step-roms-description = Поместите ваши файлы .gba от Battle Network / Rockman EXE в:
welcome-step-roms-detected = Обнаружено { $count } ROM.
welcome-step-nickname = Задайте свой ник
welcome-step-nickname-description = Можно изменить в любой момент в Настройках.
welcome-roms-needed = Добавьте хотя бы один ROM, чтобы продолжить.
rescan = Пересканировать
