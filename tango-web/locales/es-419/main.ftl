## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = Español (Latinoamérica)
window-quit = Salir de Tango
tab-play = Jugar
tab-replays = Repeticiones
tab-patches = Parches
tab-settings = Ajustes
play-no-game = No game selected
play-no-save = Elegir guardado
play-no-patch = No patch
play-version-placeholder = —
play-link-code = Código de conexión (déjalo vacío para generar uno aleatorio)
play-play = Jugar
play-fight = ¡Netbattle!
play-cancel = Cancelar
play-no-selection = Elige un juego y un guardado para inspeccionar.
play-status-connecting = Conectando al servidor de emparejamiento…
play-status-waiting-opponent = Esperando al rival…
play-you = Tú
play-opponent = Rival
empty-no-roms-title = No se encontraron ROMs
empty-no-roms-body = Coloca tus archivos .gba de Battle Network / Rockman EXE en:
lobby-waiting = Esperando…
lobby-latency = Ping: { $ms } ms
lobby-link-code = Código de conexión: { $code }
lobby-match-type = Tipo de partida
lobby-ready = Listo
lobby-unready = No listo
lobby-match-starting = Iniciando…
lobby-compat-ok = Compatible — listos para jugar.
lobby-compat-missing-game = Un lado no eligió juego.
lobby-compat-missing-rom = El juego o el parche no está instalado en ambos lados.
lobby-compat-match-mismatch = El tipo de partida no coincide.
lobby-pick-game-first = Primero elige un juego
lobby-no-match-types = (no hay tipos de partida para este juego)
session-results-victory = ¡Victoria!
session-results-defeat = Derrota
session-results-draw = Empate
session-results-no-contest = Partida finalizada
session-results-vs = vs { $nickname }
session-results-you = Tú
session-results-round = Ronda { $number }
session-results-draws = { $count ->
    [one] 1 ronda terminó en empate
   *[other] { $count } rondas terminaron en empate
}
session-results-done = Listo
discord-presence-in-single-player = En partida de un jugador
discord-presence-in-progress = Partida en progreso
playback-pause = Pausar
playback-close = Cerrar
replays-watch = Ver
replays-incomplete = incompleto
replays-watch-missing-rom = Ver (el ROM de este juego no está escaneado)
save-delete = Eliminar
patches-update = Actualizar
patches-updating = Actualizando…
patches-update-failed = Falló la actualización: { $error }
patches-netplay-compatibility = Compatibilidad de netplay:
patches-details-games = Juegos compatibles:
patches-select-prompt = Selecciona un parche.
settings-patch-repo = Repositorio de parches
settings-section-general = General
settings-section-graphics = Gráficos
settings-section-audio = Audio
settings-section-input = Entrada
settings-section-about = Acerca de
settings-volume = Volumen
settings-nickname = Apodo
settings-language = Lenguaje
settings-input-press-key = Presiona una tecla o botón…
settings-input-reset = Restablecer predeterminados
settings-input-select-hint = Haz clic en un botón para editar sus asignaciones
welcome-title = ¡Bienvenido a Tango!
welcome-subtitle = Solo faltan unos pasos para que puedas empezar a jugar.
welcome-continue = ¡Estoy listo/a!
welcome-step-roms = Agrega tus ROMs
welcome-step-roms-detected = { $count } ROMs detectados.
welcome-step-nickname = Pon tu apodo
welcome-roms-needed = Agrega al menos un ROM para continuar.

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = NaviCust
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-empty = Este guardado no tiene datos para esta vista.
save-copy = Copiar
save-copy-image = Copiar como imagen
copied = ¡Copiado!
save-edit = Editar
save-edit-save = Guardar
save-edit-cancel = Cancelar
save-edit-sort = Ordenar
save-edit-clear = Borrar todo
folder-group = Agrupar por chip
navi-style = Estilo
navi-style-unset = (sin estilo)
navi-id = ID de Navi
navi-link-navi = Link Navi
navi-base-hp = HP
navi-buster = Buster
navi-buster-attack = Ataque
navi-buster-rapid = Rapidez
navi-buster-charge = Carga
navi-power-attack = Ataque de poder
navi-edit-select = Navi
folder-edit-search = Buscar chips…
folder-edit-folder = Folder
folder-edit-count = { $count } / { $limit }
folder-edit-navi = Navi { $used } / { $limit }
folder-edit-mega = Mega { $used } / { $limit }
folder-edit-giga = Giga { $used } / { $limit }
folder-edit-dark = Dark { $used } / { $limit }
folder-edit-reg-memory = Reg { $mb }MB
folder-edit-tag-memory = Tag { $mb }MB
folder-sort-id = ID
folder-sort-name = Nombre
folder-sort-code = Código
folder-sort-attack = Ataque
folder-sort-element = Elemento
folder-sort-mb = MB
navicust-grid-size = Cuadrícula: { $cols } × { $rows }
navicust-parts = Piezas instaladas
navicust-empty = (ninguna instalada)
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
save-actions = Acciones de guardado
save-duplicate = Duplicar
save-rename = Renombrar
save-rename-confirm = Renombrar
save-delete-confirm = Eliminar
save-action-cancel = Cancelar
save-delete-prompt = ¿Eliminar { $name }?
save-name-placeholder = Nombre nuevo
save-new = Guardado nuevo
save-new-confirm = Crear
save-template-default = (predeterminado)
save-template-pick = Elegir una plantilla…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = Todos los juegos
replays-filter-any-time = Cualquier fecha
replays-filter-past-day = Últimas 24 horas
replays-filter-past-week = Última semana
replays-filter-past-month = Último mes
replays-filter-past-year = Último año
replays-filter-search-placeholder = Buscar repeticiones…
replays-show-incomplete = Mostrar incompletas
replays-direct-marker = (directo)
replays-select-prompt = Selecciona una repetición.
replays-match-type = Tipo de partida:
replays-duration = Duración:
replays-round-count = { $count ->
    [one] 1 ronda
   *[other] { $count } rondas
}

## patches detail (extracted from the desktop's main.ftl; keep in sync)
patches-favorite = Favorito
patches-unfavorite = Quitar favorito
patches-search-placeholder = Buscar parches…
patches-readme-placeholder = Este parche no tiene README.
patches-details-authors = Autores:
patches-details-license = Licencia:
    .all-rights-reserved = Todos los derechos reservados
patches-details-source = Sitio web:

## netplay settings (extracted from the desktop's main.ftl; keep in sync)
settings-matchmaking-endpoint = Salida de emparejamiento
settings-use-relay = Usar servidor relay
settings-use-relay-auto = Automático
settings-use-relay-always = Siempre
settings-use-relay-never = Nunca
settings-show-opponent-setup = Mostrar la configuración del rival al iniciar la partida

## netplay settings section label (extracted from the desktop)
settings-section-netplay = Netplay

## accent + patch repo settings (extracted from the desktop)
settings-accent = Color de acento
settings-accent-tango-green = Verde Tango
settings-accent-megaman-blue = Azul MegaMan
settings-accent-protoman-red = Rojo ProtoMan
settings-accent-roll-pink = Rosa Roll
settings-accent-gutsman-yellow = Amarillo GutsMan
settings-accent-bass-purple = Morado Bass

## welcome step description (extracted from the desktop)
welcome-step-nickname-description = Puedes cambiarlo en cualquier momento en Ajustes.

## replay video export (extracted from the desktop)
replays-export = Exportar
replays-export-progress = Renderizando…
replays-export-cancel = Cancelar
replays-export-success = Renderización completada.
replays-export-error = Renderizado fallido: { $error }

## theme + streamer + autoupdate settings (extracted from the desktop)
settings-theme = Tema
settings-theme-dark = Oscuro
settings-theme-light = Claro
settings-streamer-mode = Modo de privacidad del streamer
settings-enable-patch-autoupdate = Auto actualización

## mute bgm setting (extracted from the desktop)
settings-disable-bgm-in-pvp = Deshabilitar música en netplay

## cover tab (extracted from the desktop)
save-tab-cover = Cubierta

## lobby + telemetry + transport (extracted from the desktop)
lobby-blind-mine = Ocultar configuración
lobby-blind-self-on = Estás ocultando tu configuración.
lobby-blind-peer-on = El rival está ocultando su configuración.
settings-netplay-frame-delay = Retardo de fotogramas
lobby-frame-delay-suggest = Sugerir según el ping
session-stat-tps = Tick/s (actual/máx.)
session-stat-skew = Desfase
session-stat-lead = Ventaja
session-stat-depth = Profundidad de predicción errónea
session-stat-ping = Latencia de red
playback-speed = Velocidad
playback-input-display = Mostrar entradas

## replay input display + swap (extracted from the desktop)
playback-swap-perspective = Perspectiva del oponente
