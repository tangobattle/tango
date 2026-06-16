# Window
# Endonym for this locale; shown in the language picker.
LANGUAGE = 한국어

window-title = Tango
# Tooltip on the top bar's close button (fullscreen only).
window-quit = Tango 종료

# Crash handler dialogs (parent process)
crash = Tango가 오류로 종료되었습니다.

    버그 보고 시 다음 로그 파일을 첨부해주세요:

    { $path }
crash-no-log = Tango가 오류로 종료되었습니다.

    { $error }

# Discord rich presence
discord-presence-looking = 대전 상대 찾는 중
discord-presence-in-single-player = 싱글 플레이 중
discord-presence-in-lobby = 로비 대기 중
discord-presence-in-progress = 대전 중

# Top-bar tabs
tab-play = 대전
tab-replays = 리플레이
tab-patches = 패치
tab-settings = 설정

# Play selectors
play-no-game = 게임 미선택
play-no-save = 세이브 선택

# Save management
save-open-folder = 폴더 열기
save-duplicate = 복제
save-rename = 이름 변경
save-delete = 삭제
save-rename-confirm = 이름 변경
save-delete-confirm = 삭제
save-action-cancel = 취소
save-delete-prompt = { $name }을(를) 삭제하시겠습니까?
save-name-placeholder = 새 이름
save-new = 새 세이브
save-new-confirm = 생성
save-template-default = (기본값)
save-template-pick = 템플릿 선택…

# Empty-state hints
empty-no-roms-title = ROM이 없습니다
empty-no-roms-body = 배틀 네트워크 록맨 에그제의 .gba 파일을 다음 위치에 놓으세요:
empty-no-saves-title = 이 게임의 세이브가 없습니다
empty-no-saves-body = 이 게임의 .sav 파일을 다음 위치에 놓으세요:
play-no-patch = 패치 없음
play-patch-toggle = 패치 사용…
play-version-placeholder = —

# Play bottom strip
play-link-code = 링크 코드 (비워두면 랜덤)
play-link-code-random = 랜덤 링크 코드
play-play = 플레이
play-fight = 대전
play-cancel = 취소
play-status-idle = 네트플레이를 시작하려면 링크 코드를 입력하거나, 비워두면 싱글 플레이 모드입니다.
play-status-connecting = 매치메이킹 서버에 연결 중…
play-status-direct-connecting = 대전 상대에 연결 중…
play-status-waiting-opponent = 대전 상대를 기다리는 중…
play-status-negotiating = 협상 중…
play-status-failed = 연결 실패: { $error }
play-status-peer-disconnected = 상대가 나갔습니다.
play-status-negotiate-expected-hello = 상대에서 예상된 핸드셰이크를 받지 못했습니다.
play-status-negotiate-version-too-old = 상대가 이전 버전의 Tango를 사용하고 있습니다.
play-status-negotiate-version-too-new = 상대가 새로운 버전의 Tango를 사용하고 있습니다.
play-status-negotiate-failed = 협상 중 오류가 발생했습니다: { $error }
lobby-waiting = 대기 중…
lobby-no-game = (게임 미선택)
lobby-latency = Ping: { $ms } ms
lobby-latency-direct = Ping (직접): { $ms } ms
lobby-latency-relayed = Ping (중계): { $ms } ms
lobby-link-code = 링크 코드: { $code }
lobby-direct-host = UDP 포트 { $port }에서 호스트 중
lobby-direct-connect = UDP를 통해 { $target }에 연결 중
lobby-handshake = 설정 교환 중…
lobby-match-type = 매치 타입
lobby-frame-delay-suggest = Ping을 기반으로 추천
lobby-no-match-types = (이 게임에는 대전 모드가 없습니다)
lobby-pick-game-first = 먼저 게임을 선택하세요

lobby-compat-ok = 호환됨 — 대전할 수 있습니다.
lobby-compat-missing-game = 한 쪽이 게임을 선택하지 않았습니다.
lobby-compat-missing-rom = 한쪽 또는 둘 다 게임이나 패치가 설치되지 않았습니다.
lobby-compat-version-mismatch = 게임 버전이 일치하지 않습니다 (다른 패치 / ROM).
lobby-compat-match-mismatch = 매치 타입이 일치하지 않습니다.
lobby-ready = 준비 완료
lobby-unready = 준비 취소
lobby-match-starting = 시작 중…
lobby-blind-mine = 구성 숨기기
lobby-blind-peer-on = 상대가 구성을 숨기고 있습니다.
lobby-blind-self-on = 자신의 구성을 숨기고 있습니다.
session-opponent = 상대 빌드
session-self = 내 빌드
session-back-to-session = 대전으로 돌아가기
# PvP telemetry deck cell tooltips
session-stat-tps = 틱/초 (현재/최대)
session-stat-skew = 스큐
session-stat-depth = 오측 깊이
session-stat-ping = 네트워크 지연

# Save view sub-tabs
save-tab-cover = 커버
save-tab-navi = 나비
save-tab-folder = 폴더
save-tab-patch-cards = 패치 카드
save-tab-auto-battle-data = 자동 배틀 데이터

# Navi pane
navi-style = 스타일

# Folder pane
folder-group = 칩으로 정렬
save-copy = 복사
copied = 복사했습니다!
save-copy-image = 이미지로 복사

# Navi pane
navi-id = 나비 ID
navi-link-navi = 링크 나비
navi-style-unset = (스타일 없음)
navicust-grid-size = 그리드: { $cols } × { $rows }
navicust-parts = 설치된 부품
navicust-empty = (미설치)

# Folder editor
save-edit = 편집
save-edit-save = 저장
save-edit-cancel = 취소
folder-edit-search = 칩 검색…
folder-edit-folder = 폴더
folder-edit-count = { $count } / { $limit }
folder-edit-navi = 나비 { $used } / { $limit }
folder-edit-mega = 메가 { $used } / { $limit }
folder-edit-giga = 기가 { $used } / { $limit }
folder-edit-dark = 다크 { $used } / { $limit }
folder-edit-reg-memory = 레귤러 { $mb }MB
folder-edit-tag-memory = 태그 { $mb }MB
save-edit-sort = 정렬
save-edit-clear = 모두 지우기
folder-sort-id = ID
folder-sort-name = 이름
folder-sort-code = 코드
folder-sort-attack = 공격력
folder-sort-element = 속성
folder-sort-mb = MB

# Navicust editor
navicust-edit-grid = 나비 커스터마이저
navicust-edit-count = { $count } 부품
navicust-edit-rotate = 회전
navicust-edit-compress = 압축
navicust-edit-uncompress = 압축 해제
navicust-edit-search = 부품 검색…
navicust-sort-id = ID
navicust-sort-name = 이름
navicust-sort-color = 색상

# Patch card editor
patch-card-edit-search = 카드 검색…
patch-card-edit-count = { $count } 장
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = 이름
patch-card-sort-mb = MB
patch-card4-none = 없음

# Auto Battle Data pane
auto-battle-data-secondary-standard-chips = 스탠더드 칩 (보조)
auto-battle-data-standard-chips = 스탠더드 칩
auto-battle-data-mega-chips = 메가칩
auto-battle-data-giga-chip = 기가칩
auto-battle-data-combos = 콤보
auto-battle-data-program-advance = 프로그램 어드밴스

# Auto Battle Data editor
auto-battle-data-edit-used = 사용 횟수
auto-battle-data-edit-secondary = 보조
auto-battle-data-edit-count = { $count } 장

# Common
save-empty = 이 세이브에는 이 보기의 데이터가 없습니다.
play-no-selection = 검사할 게임과 세이브를 선택하세요.

# Replays
replays-filter-all-games = 모두
replays-filter-opponent-placeholder = 검색
replays-show-incomplete = 미완료도 표시
replays-direct-marker = (직접)
replays-watch = 재생
replays-watch-missing-rom = 재생 (이 게임의 ROM이 스캔되지 않음)
replays-export = 녹화
replays-export-progress = 녹화 중…
replays-export-cancel = 취소
replays-export-cancelling = 취소 중…
replays-export-success = 녹화가 완료되었습니다.
replays-export-error = 녹화에 실패했습니다: { $error }
replays-export-open = 동영상 열기
replays-export-reset = 리셋
replays-export-scale = 확대율
replays-export-scale-lossless = 무손실
replays-export-disable-bgm = 음악 끄기
replays-export-twosided = 양면 표시
replays-export-rounds = 라운드:
replays-export-save-as = 다른 이름으로 저장…
playback-close = 닫기
playback-options = 옵션
playback-speed = 속도
playback-play = 재생
playback-pause = 일시정지
playback-disconnect = 연결 끊기
playback-disconnect-prompt = 이 경기에서 연결을 끊으시겠습니까?
playback-disconnect-detail = 상대와의 경기를 종료합니다.
playback-cancel = 취소
replays-select-prompt = 리플레이를 선택하세요.
play-opponent = 상대
replays-match-type = 매치 타입:
replays-duration = 재생 시간:
replays-round-count = { $count }라운드
replays-incomplete = 미완료
play-you = 자신

# Patches
patches-update = 업데이트
patches-updating = 업데이트 중…
patches-update-failed = 업데이트 실패: { $error }
patches-open-folder = 폴더 열기
patches-favorite = 즐겨찾기
patches-unfavorite = 즐겨찾기 해제
patches-search-placeholder = 패치 검색…
patches-select-prompt = 패치를 선택하세요.
patches-readme-placeholder = 이 패치에는 README가 없습니다.
patches-details-authors = 작가:
patches-details-license = 라이선스:
patches-details-source = 소스:
patches-details-games = 지원 게임:
patches-netplay-compatibility = 네트플레이 호환성:

# Settings panel
settings-section-general = 일반
settings-section-graphics = 그래픽
settings-section-netplay = 네트플레이
settings-section-audio = 오디오
settings-volume = 볼륨
settings-disable-bgm-in-pvp = 네트플레이에서 음악 끄기
settings-nickname = 닉네임
settings-language = 언어
settings-data-path = 데이터 경로
settings-streamer-mode = 스트림 개인 정보 보호 모드
settings-section-experimental = 실험적 기능
settings-enable-save-editor = 세이브 에디터 활성화
settings-experimental-warning = 실험적 기능은 세이브를 손상시키거나 사용 불가능하게 만들 수 있으며, 예고 없이 변경 또는 삭제될 수 있습니다. 또한 온라인 플레이에서 세이브를 유효 상태로 유지하는 확인이 생략될 수 있습니다. 자신의 책임하에 사용하세요.
settings-section-about = 앱 정보
settings-section-input = 입력
settings-input-press-key = 키 또는 버튼을 누르세요…
settings-input-add = 할당 추가
settings-input-reset = 기본값으로 재설정
input-key-up = 위
input-key-down = 아래
input-key-left = 왼쪽
input-key-right = 오른쪽
input-key-a = A
input-key-b = B
input-key-l = L
input-key-r = R
input-key-start = 스타트
input-key-select = 셀렉트
input-key-speed-up = 빨리감기
input-gamepad-south = A 버튼
input-gamepad-east = B 버튼
input-gamepad-west = X 버튼
input-gamepad-north = Y 버튼
input-gamepad-select = 셀렉트
input-gamepad-start = 스타트
input-gamepad-mode = 가이드
input-gamepad-left-thumb = 왼쪽 스틱
input-gamepad-right-thumb = 오른쪽 스틱
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = 십자키 위
input-gamepad-dpad-down = 십자키 아래
input-gamepad-dpad-left = 십자키 왼쪽
input-gamepad-dpad-right = 십자키 오른쪽
input-gamepad-misc1 = 기타 1
input-gamepad-misc2 = 기타 2
input-gamepad-misc3 = 기타 3
input-gamepad-misc4 = 기타 4
input-gamepad-misc5 = 기타 5
input-gamepad-misc6 = 기타 6
input-gamepad-right-paddle1 = 오른쪽 패들 1
input-gamepad-left-paddle1 = 왼쪽 패들 1
input-gamepad-right-paddle2 = 오른쪽 패들 2
input-gamepad-left-paddle2 = 왼쪽 패들 2
input-gamepad-touchpad = 터치패드
input-gamepad-axis-left-stick-x = 왼쪽 스틱 X
input-gamepad-axis-left-stick-y = 왼쪽 스틱 Y
input-gamepad-axis-right-stick-x = 오른쪽 스틱 X
input-gamepad-axis-right-stick-y = 오른쪽 스틱 Y
input-gamepad-axis-trigger-left = 왼쪽 트리거
input-gamepad-axis-trigger-right = 오른쪽 트리거
settings-theme = 테마
settings-theme-dark = 다크
settings-theme-light = 라이트
settings-matchmaking-endpoint = 매치메이킹 엔드포인트
settings-patch-repo = 패치 리포지토리
settings-enable-patch-autoupdate = 백그라운드에서 패치 자동 업데이트
settings-enable-updater = 앱 업데이트 자동 확인
settings-allow-prerelease-upgrades = 앱 업데이트 확인 시 프리릴리스 포함
settings-netplay-frame-delay = 프레임 지연
settings-use-relay = 릴레이 서버 사용
settings-use-relay-auto = 자동
settings-use-relay-always = 항상 사용
settings-use-relay-never = 사용 안함
settings-window-size = 창 크기
settings-fullscreen = 전체 화면
settings-ui-scale = UI 배율
settings-fractional-scaling = 분수 스케일링
settings-hide-emulator-border = 에뮬레이터 경계 숨기기
settings-video-filter = 비디오 필터
updater-current-version = 현재 버전: { $version }
updater-latest-version = 최신 버전: { $version }
updater-loading = 확인 중…
updater-up-to-date = v{ $version } (최신)
updater-downloading = 다운로드 중: { $pct }%
updater-ready-to-update = 업데이트 준비가 완료되었습니다.
updater-update-now = 지금 업데이트

# Welcome screen
welcome-title = Tango에 오신 것을 환영합니다!
welcome-subtitle = 대전하기 전에 몇 가지 초기 설정을 완료해주세요.
welcome-continue = 계속
welcome-step-roms = ROM 추가
welcome-step-roms-description = 배틀 네트워크 록맨 에그제의 .gba 파일을 다음 위치에 놓으세요:
welcome-step-roms-detected = { $count }개의 ROM을 감지했습니다.
welcome-step-nickname = 닉네임 설정
welcome-step-nickname-description = 설정에서 언제든지 변경할 수 있습니다.
welcome-open-folder = ROM 폴더 열기
welcome-roms-needed = 계속하기 전에 1개 이상의 ROM을 추가하세요.

# Common actions
rescan = 다시 스캔

# Game names live in games.ftl (Fluent attribute scheme shared with
# the legacy app: game-<family>.variant-N, .short, .match-type-X-Y).
