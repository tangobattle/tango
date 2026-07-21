## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = Português (Brasil)
window-quit = Sair do Tango
tab-play = Jogar
tab-replays = Replays
tab-patches = Patches
tab-settings = Ajustes
play-no-game = Nenhum jogo selecionado
play-no-save = Escolher save
play-no-patch = Nenhum patch selecionado
play-version-placeholder = —
play-link-code = Código de conexão (deixe vazio para gerar um aleatório)
play-play = Jogar
play-fight = Batalhar!
play-cancel = Cancelar
play-no-selection = Selecione um jogo e um save para inspecionar.
play-status-connecting = Conectando ao servidor de matchmaking…
play-status-waiting-opponent = Aguardando oponente…
play-you = Você
play-opponent = Oponente
empty-no-roms-title = Nenhuma ROM encontrada
empty-no-roms-body = Coloque seus arquivos .gba de Battle Network / Rockman EXE em:
lobby-waiting = Aguardando…
lobby-latency = Ping: { $ms } ms
lobby-link-code = Código de conexão: { $code }
lobby-match-type = Tipo de partida
lobby-ready = Pronto
lobby-unready = Não pronto
lobby-match-starting = Iniciando…
lobby-compat-ok = Compatível — pronto para jogar.
lobby-compat-missing-game = Um lado não escolheu jogo.
lobby-compat-missing-rom = Jogo ou patch não está instalado em ambos os lados.
lobby-compat-match-mismatch = Tipo de partida não bate.
lobby-pick-game-first = Escolha um jogo primeiro
lobby-no-match-types = (sem tipos de partida para este jogo)
session-results-victory = Vitória!
session-results-defeat = Derrota
session-results-draw = Empate
session-results-no-contest = Partida encerrada
session-results-vs = vs { $nickname }
session-results-you = Você
session-results-round = Round { $number }
session-results-draws = { $count ->
    [one] 1 round terminou empatado
   *[other] { $count } rounds terminaram empatados
}
session-results-done = Concluído
discord-presence-in-single-player = No singleplayer
discord-presence-in-progress = Partida em progresso
playback-pause = Pausar
playback-close = Fechar
replays-watch = Assistir
replays-incomplete = incompleto
replays-watch-missing-rom = Assistir (ROM deste jogo não foi escaneada)
save-delete = Excluir
patches-update = Atualização
patches-updating = Atualizando…
patches-update-failed = Falha na atualização: { $error }
patches-netplay-compatibility = Compatibilidade de netplay:
patches-details-games = Jogos compatíveis:
patches-select-prompt = Selecione um patch.
settings-patch-repo = Repositório de patches
settings-section-general = Geral
settings-section-graphics = Gráficos
settings-section-audio = Áudio
settings-section-input = Entrada
settings-section-about = Sobre
settings-volume = Volume
settings-nickname = Apelido
settings-language = Idioma
settings-input-press-key = Pressione uma tecla ou botão…
settings-input-reset = Restaurar padrões
settings-input-select-hint = Clique em um botão para editar seus atalhos
welcome-title = Bem-vindo ao Tango!
welcome-subtitle = Faltam só alguns passos antes de começar a jogar.
welcome-continue = Terminei!
welcome-step-roms = Adicione suas ROMs
welcome-step-roms-detected = { $count } ROMs detectadas.
welcome-step-nickname = Defina seu apelido
welcome-roms-needed = Adicione pelo menos uma ROM para continuar.

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = NaviCust
save-tab-folder = Folder
save-tab-patch-cards = Patch Cards
save-tab-auto-battle-data = Auto Battle Data
save-empty = Este save não tem dados para esta visualização.
save-copy = Copiar
save-copy-image = Copiar como imagem
copied = Copiado!
save-edit = Editar
save-edit-save = Salvar
save-edit-cancel = Cancelar
save-edit-sort = Ordenar
save-edit-clear = Limpar tudo
folder-group = Agrupar por chip
navi-style = Estilo
navi-style-unset = (sem estilo)
navi-id = ID do Navi
navi-link-navi = Link Navi
navi-base-hp = HP
navi-buster = Buster
navi-buster-attack = Ataque
navi-buster-rapid = Rapidez
navi-buster-charge = Carga
navi-power-attack = Ataque poderoso
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
folder-sort-name = Nome
folder-sort-code = Código
folder-sort-attack = Ataque
folder-sort-element = Elemento
folder-sort-mb = MB
navicust-grid-size = Grade: { $cols } × { $rows }
navicust-parts = Peças instaladas
navicust-empty = (nenhuma instalada)
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
save-actions = Ações do save
save-duplicate = Duplicar
save-rename = Renomear
save-rename-confirm = Renomear
save-delete-confirm = Excluir
save-action-cancel = Cancelar
save-delete-prompt = Excluir { $name }?
save-name-placeholder = Novo nome
save-new = Novo save
save-new-confirm = Criar
save-template-default = (padrão)
save-template-pick = Escolher um modelo…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = Todos os jogos
replays-filter-any-time = Qualquer data
replays-filter-past-day = Últimas 24 horas
replays-filter-past-week = Última semana
replays-filter-past-month = Último mês
replays-filter-past-year = Último ano
replays-filter-search-placeholder = Pesquisar replays…
replays-show-incomplete = Mostrar incompletos
replays-direct-marker = (direto)
replays-select-prompt = Selecione um replay.
replays-match-type = Tipo de partida:
replays-duration = Duração:
replays-round-count = { $count ->
    [one] 1 round
   *[other] { $count } rounds
}
