# Endonym for this locale; shown in the language picker.
LANGUAGE = Español (Latinoamérica)

crash =
    ¡Oops, Tango ha encontrado un error y se ha estrellado!

    Cuando informe de este fallo, incluya el siguiente archivo de registro:

    { $path }
crash-no-log =
    ¡Oops, Tango ha encontrado un error y se ha estrellado!

    { $error }
window-title = Tango
    .running = Tango (en ejecución)
play-play = Jugar
play-fight = ¡Netbattle!
play-link-code = Código de conexión
play-no-game = No game selected
play-no-patch = No patch
play-you = Tú
play-cancel = Cancelar
replays-export = Exportar
replays-export-disable-bgm = Deshabilitar música
replays-export-twosided = Dos lados
replays-export-success = Renderización completada.
replays-export-error = Renderizado fallido: { $error }
replays-export-open = Open
patches-open-folder = Abrir carpeta
patches-favorite = Favorito
patches-unfavorite = Quitar favorito
patches-search-placeholder = Buscar parches…
patches-update = Actualizar
patches-details-authors = Autores:
patches-details-license = Licencia:
    .all-rights-reserved = Todos los derechos reservados
patches-details-source = Sitio web:
patches-details-games = Juegos compatibles:
settings-theme = Tema
settings-theme-dark = Oscuro
settings-theme-light = Claro
    .light = Claro
    .dark = Oscuro
    .system = Seguir la configuración del sistema
settings-video-filter = Filtro de vídeo
    .null = Ninguno
    .hq2x = hq2x
    .hq3x = hq3x
    .hq4x = hq4x
    .mmpx = MMPX
settings-language = Lenguaje
settings-nickname = Apodo
settings-streamer-mode = Modo de privacidad del streamer
settings-section-experimental = Experimental
settings-enable-save-editor = Habilitar editor de guardados
settings-experimental-warning = Las funciones experimentales pueden dañar o corromper tus guardados, pueden cambiar o eliminarse en cualquier momento y pueden carecer de comprobaciones que mantengan tus guardados válidos para el juego en línea. Úsalas bajo tu propio riesgo.
    .tooltip = Si activas este modo, se añadirá una pestaña adicional de "Cubierta" al visor de guardado que oculta toda la información sobre tu archivo de guardado actual.
settings-matchmaking-endpoint = Salida de emparejamiento
settings-patch-repo = Repositorio de parches
settings-enable-patch-autoupdate = Auto actualización
settings-data-path = Ruta de datos
    .open = Abrir
    .change = Cambiar
settings-window-size = Tamaño de ventana
settings-fullscreen = Pantalla completa
settings-ui-scale = Escala de interfaz
settings-fractional-scaling = Escalado fraccional
settings-hide-emulator-border = Ocultar borde del emulador
save-tab-cover = Cubierta
save-tab-navi = Navi
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
auto-battle-data-secondary-standard-chips = Standard chips (secundarios)
auto-battle-data-standard-chips = Standard chips
auto-battle-data-mega-chips = Mega chips
auto-battle-data-giga-chip = Giga chip
auto-battle-data-combos = Combos
auto-battle-data-program-advance = Program advance

# Auto Battle Data editor
auto-battle-data-edit-used = Usos
auto-battle-data-edit-secondary = Sec.
auto-battle-data-edit-count = { $count ->
    [one] { $count } chip
   *[other] { $count } chips
}
welcome-open-folder = Abrir carpeta
welcome-continue = ¡Estoy listo/a!
discord-presence-looking = Buscando pelea
discord-presence-in-single-player = En partida de un jugador
discord-presence-in-lobby = En sala
discord-presence-in-progress = Partida en progreso

# === translations ===
tab-play = Jugar
tab-replays = Repeticiones
tab-patches = Parches
tab-settings = Ajustes
play-no-save = Elegir guardado
save-open-folder = Abrir carpeta
save-duplicate = Duplicar
save-rename = Renombrar
save-delete = Eliminar
save-rename-confirm = Guardar
save-delete-confirm = Eliminar
save-action-cancel = Cancelar
save-delete-prompt = ¿Eliminar este guardado?
save-name-placeholder = Nombre nuevo
save-new = Guardado nuevo
save-new-confirm = Crear
save-template-default = (predeterminado)
save-template-pick = Elegir una plantilla…
empty-no-roms-title = No se encontraron ROMs
empty-no-roms-body = Coloca tus archivos .gba de Battle Network / Rockman EXE en:
empty-no-saves-title = Sin guardados para este juego
empty-no-saves-body = Coloca un .sav para este juego en:
play-version-placeholder = —
play-link-code-random = Código aleatorio
play-status-idle = Ingresa un código para iniciar netplay, o deja en blanco para un jugador.
play-status-connecting = Conectando al servidor de emparejamiento…
play-status-direct-connecting = Conectando al rival…
play-status-waiting-opponent = Esperando al rival…
play-status-negotiating = Negociando…
play-status-failed = Conexión fallida: { $error }
play-status-peer-disconnected = El otro jugador se fue.
play-status-negotiate-expected-hello = El otro jugador no envió el saludo esperado.
play-status-negotiate-version-too-old = El otro jugador está usando una versión más antigua de Tango.
play-status-negotiate-version-too-new = El otro jugador está usando una versión más nueva de Tango.
play-status-negotiate-failed = Ocurrió un error durante la negociación: { $error }
lobby-waiting = Esperando…
lobby-no-game = (sin juego seleccionado)
lobby-latency = Ping: { $ms } ms
lobby-link-code = Código de conexión: { $code }
lobby-direct-host = Alojando en el puerto: { $port }
lobby-direct-connect = Conectando a: { $target }
lobby-handshake = Intercambiando ajustes…
lobby-match-type = Tipo de partida
settings-netplay-frame-delay = Retardo de fotogramas
lobby-frame-delay-suggest = Sugerir según el ping
lobby-no-match-types = (no hay tipos de partida para este juego)
lobby-pick-game-first = Primero elige un juego
lobby-compat-ok = Compatible — listos para jugar.
lobby-compat-missing-game = Un lado no eligió juego.
lobby-compat-missing-rom = El juego o el parche no está instalado en ambos lados.
lobby-compat-version-mismatch = Las versiones del juego no coinciden (parche / ROM distintos).
lobby-compat-match-mismatch = El tipo de partida no coincide.
lobby-ready = Listo
lobby-unready = No listo
lobby-match-starting = Iniciando…
lobby-reveal-mine = Mostrar mi configuración al rival
lobby-reveal-peer-on = El rival está mostrando su configuración.
lobby-reveal-peer-off = El rival no la está mostrando.
lobby-reveal-peer-unknown = (esperando al rival)
session-opponent = Configuración del rival
session-self = Mi configuración
session-back-to-session = Volver a la sesión
# PvP telemetry deck cell tooltips
session-stat-tps = Tick/s (actual/máx.)
session-stat-skew = Desfase
session-stat-depth = Profundidad de predicción errónea
session-stat-ping = Latencia de red
navi-style = Estilo
folder-group = Agrupar por chip
save-copy = Copiar
save-copy-image = Copiar como imagen
navi-id = ID de Navi
navi-style-unset = (sin estilo)
navicust-grid-size = Cuadrícula: { $cols } × { $rows }
navicust-parts = Piezas instaladas
navicust-empty = (ninguna instalada)

# Folder editor
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
save-edit-sort = Ordenar
save-edit-clear = Borrar todo
folder-sort-id = ID
folder-sort-name = Nombre
folder-sort-code = Código
folder-sort-attack = Ataque
folder-sort-element = Elemento
folder-sort-mb = MB

# Navicust editor
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

# Patch card editor
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
play-no-selection = Elige un juego y un guardado para inspeccionar.
replays-filter-all-games = Todos los juegos
replays-filter-opponent-placeholder = Cualquiera
replays-show-incomplete = Mostrar incompletas
replays-direct-marker = (directo)
replays-watch = Ver
replays-watch-missing-rom = Ver (el ROM de este juego no está escaneado)
replays-export-progress = Renderizando…
replays-export-cancel = Cancelar
replays-export-cancelling = Cancelando…
replays-export-reset = Restablecer
replays-export-scale = Escala
replays-export-scale-lossless = sin pérdida
replays-export-rounds = Rondas:
replays-export-save-as = Guardar como…
playback-close = Cerrar
playback-options = Opciones
playback-speed = Velocidad
playback-play = Reproducir
playback-pause = Pausar
playback-disconnect = Desconectar
playback-disconnect-prompt = ¿Desconectarse de esta partida?
playback-disconnect-detail = Terminarás la partida con tu rival.
playback-cancel = Cancelar
replays-select-prompt = Selecciona una repetición.
play-opponent = Rival
replays-match-type = Tipo de partida:
replays-duration = Duración:
replays-round-count = { $count ->
    [one] 1 ronda
   *[other] { $count } rondas
}
replays-incomplete = incompleto
patches-updating = Actualizando…
patches-update-failed = Falló la actualización: { $error }
patches-select-prompt = Selecciona un parche.
patches-readme-placeholder = Este parche no tiene README.
patches-netplay-compatibility = Compatibilidad de netplay:
settings-section-general = General
settings-section-graphics = Gráficos
settings-section-netplay = Netplay
settings-section-audio = Audio
settings-volume = Volumen
settings-section-about = Acerca de
settings-section-input = Entrada
settings-input-press-key = Presiona una tecla o botón…
settings-input-add = Agregar asignación
settings-input-reset = Restablecer predeterminados
input-key-up = Arriba
input-key-down = Abajo
input-key-left = Izquierda
input-key-right = Derecha
input-key-a = A
input-key-b = B
input-key-l = L
input-key-r = R
input-key-start = Start
input-key-select = Select
input-key-speed-up = Avance rápido
input-gamepad-south = Botón A
input-gamepad-east = Botón B
input-gamepad-west = Botón X
input-gamepad-north = Botón Y
input-gamepad-select = Select
input-gamepad-start = Start
input-gamepad-mode = Guía
input-gamepad-left-thumb = Stick izquierdo
input-gamepad-right-thumb = Stick derecho
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = Cruceta arriba
input-gamepad-dpad-down = Cruceta abajo
input-gamepad-dpad-left = Cruceta izquierda
input-gamepad-dpad-right = Cruceta derecha
input-gamepad-misc1 = Misc 1
input-gamepad-misc2 = Misc 2
input-gamepad-misc3 = Misc 3
input-gamepad-misc4 = Misc 4
input-gamepad-misc5 = Misc 5
input-gamepad-misc6 = Misc 6
input-gamepad-right-paddle1 = Paleta derecha 1
input-gamepad-left-paddle1 = Paleta izquierda 1
input-gamepad-right-paddle2 = Paleta derecha 2
input-gamepad-left-paddle2 = Paleta izquierda 2
input-gamepad-touchpad = Panel táctil
input-gamepad-axis-left-stick-x = Stick izquierdo X
input-gamepad-axis-left-stick-y = Stick izquierdo Y
input-gamepad-axis-right-stick-x = Stick derecho X
input-gamepad-axis-right-stick-y = Stick derecho Y
input-gamepad-axis-trigger-left = Gatillo izquierdo
input-gamepad-axis-trigger-right = Gatillo derecho
settings-enable-updater = Buscar actualizaciones de la app automáticamente
settings-allow-prerelease-upgrades = Incluir versiones preliminares al buscar actualizaciones
updater-current-version = Versión actual: { $version }
updater-latest-version = Última versión: { $version }
updater-loading = comprobando…
updater-up-to-date = v{ $version } (al día)
updater-downloading = Descargando: { $pct }%
updater-ready-to-update = Actualización descargada y lista para instalar.
updater-update-now = Actualizar ahora
welcome-title = ¡Bienvenido a Tango!
welcome-subtitle = Solo faltan unos pasos para que puedas empezar a jugar.
welcome-step-roms = Agrega tus ROMs
welcome-step-roms-description = Coloca tus archivos .gba de Battle Network / Rockman EXE en:
welcome-step-roms-detected = { $count } ROMs detectados.
welcome-step-nickname = Pon tu apodo
welcome-step-nickname-description = Puedes cambiarlo en cualquier momento en Ajustes.
welcome-roms-needed = Agrega al menos un ROM para continuar.
rescan = Reanalizar
