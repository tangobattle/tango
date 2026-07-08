# Window
# Endonym for this locale; shown in the language picker.
LANGUAGE = 繁體中文（台灣、香港、澳門）

window-title = Tango
# Tooltip on the top bar's close button (fullscreen only).
window-quit = 結束 Tango

# Crash handler dialogs (parent process)
crash = 糟糕，Tango 遇到錯誤並已當機！

    回報此當機時，請附上以下記錄檔：

    { $path }
crash-no-log = 糟糕，Tango 遇到錯誤並已當機！

    { $error }

# Discord rich presence
discord-presence-looking = 正在尋找對戰
discord-presence-in-single-player = 單人遊戲中
discord-presence-in-lobby = 大廳中
discord-presence-in-progress = 對戰進行中

# Top-bar tabs
tab-play = 對戰
tab-replays = 重播
tab-patches = 補丁
tab-settings = 設定

# Play selectors
play-no-game = 未選擇遊戲
play-no-save = 選擇存檔

# Save management
save-open-folder = 開啟資料夾
save-duplicate = 建立副本
save-rename = 重新命名
save-delete = 刪除
save-rename-confirm = 重新命名
save-delete-confirm = 刪除
save-action-cancel = 取消
save-delete-prompt = 刪除 { $name }？
save-name-placeholder = 新名稱
save-new = 新增存檔
save-new-confirm = 建立
save-template-default = （預設）
save-template-pick = 選擇範本…

# Empty-state hints
empty-no-roms-title = 找不到遊戲 ROM
empty-no-roms-body = 將你的 Battle Network / Rockman EXE .gba 檔案放入：
empty-no-saves-title = 此遊戲沒有存檔檔案
empty-no-saves-body = 將此遊戲的 .sav 檔案放入：
play-no-patch = 無補丁
play-patch-toggle = 使用補丁…
play-version-placeholder = —

# Play bottom strip
play-link-code = 連線代碼（留空則隨機產生）
play-link-code-random = 隨機連線代碼
play-play = 開始
play-fight = 戰鬥
play-cancel = 離開
play-status-idle = 輸入連線代碼開始連線對戰，留空則進行單人遊戲。
play-status-connecting = 正在連線至配對伺服器…
play-status-direct-connecting = 正在連線至對手…
play-status-waiting-opponent = 正在等待對手…
play-status-negotiating = 正在協商…
play-status-failed = 連線失敗：{ $error }
play-status-peer-disconnected = 對方已離開。
play-status-negotiate-expected-hello = 對方未傳送預期的交握訊息。
play-status-negotiate-version-too-old = 對方執行的是較舊版本的 Tango。
play-status-negotiate-version-too-new = 對方執行的是較新版本的 Tango。
play-status-negotiate-failed = 協商過程中發生錯誤：{ $error }
lobby-waiting = 等待中…
lobby-no-game = （未選擇遊戲）
lobby-latency = 延遲：{ $ms } 毫秒
lobby-latency-direct = 延遲（直連）：{ $ms } 毫秒
lobby-latency-relayed = 延遲（中繼）：{ $ms } 毫秒
lobby-link-code = 連線代碼：{ $code }
lobby-direct-host = 正在 UDP 連接埠監聽：{ $port }
lobby-direct-connect = 正在透過 UDP 連線：{ $target }
lobby-handshake = 正在交換設定…
lobby-match-type = 對戰類型
lobby-frame-delay-suggest = 根據延遲建議
lobby-no-match-types = （此遊戲沒有可用的對戰類型）
lobby-pick-game-first = 請先選擇遊戲

lobby-compat-ok = 相容 — 可以開始對戰。
lobby-compat-missing-game = 有一方尚未選擇遊戲。
lobby-compat-missing-rom = 雙方並未都安裝該遊戲或補丁。
lobby-compat-version-mismatch = 遊戲版本不一致（補丁 / ROM 不同）。
lobby-compat-match-mismatch = 對戰類型不一致。
lobby-ready = 準備
lobby-unready = 取消準備
lobby-match-starting = 開始中…
lobby-blind-mine = 隱藏配置
lobby-blind-peer-on = 對手正在隱藏其配置。
lobby-blind-self-on = 你正在隱藏自己的配置。
session-opponent = 對手配置
session-self = 我的配置
session-back-to-session = 返回對戰
# PvP telemetry deck cell tooltips
session-stat-tps = 每秒影格數（目前/最大）
session-stat-skew = 時鐘偏移
session-stat-lead = 領先
session-stat-depth = 預測錯誤深度
session-stat-ping = 網路延遲

# Save view sub-tabs
save-tab-cover = 封面
save-tab-navi = 領航員
save-tab-navicust = 領航客製
save-tab-folder = 卡組
save-tab-patch-cards = 改造卡
save-tab-auto-battle-data = 自動戰鬥資料

# Navi pane
navi-style = 樣式

# Folder pane
folder-group = 依晶片分組
save-copy = 複製
copied = 已複製！
save-copy-image = 複製為圖片
save-edit = 編輯
save-edit-save = 儲存
save-edit-cancel = 取消
folder-edit-search = 搜尋晶片…
folder-edit-folder = 卡組
folder-edit-count = { $count } / { $limit }
folder-edit-navi = 領航員 { $used } / { $limit }
folder-edit-mega = MEGA { $used } / { $limit }
folder-edit-giga = GIGA { $used } / { $limit }
folder-edit-dark = 黑暗 { $used } / { $limit }
folder-edit-reg-memory = 常規 { $mb }MB
folder-edit-tag-memory = 搭檔 { $mb }MB
save-edit-sort = 排序
save-edit-clear = 全部清除
folder-sort-id = ID
folder-sort-name = 名稱
folder-sort-code = 代碼
folder-sort-attack = 攻擊力
folder-sort-element = 屬性
folder-sort-mb = MB

# Navi pane
navi-id = 領航員 ID
navi-link-navi = 連結領航員
navi-edit-select = 領航員
navi-style-unset = （無樣式）
navicust-grid-size = 格線：{ $cols } × { $rows }
navicust-parts = 已安裝的程式零件
navicust-empty = （未安裝）

# Navicust editor
navicust-edit-grid = 領航客製
navicust-edit-count = { $count } 個程式零件
navicust-edit-rotate = 旋轉
navicust-edit-compress = 壓縮
navicust-edit-uncompress = 解壓縮
navicust-edit-search = 搜尋程式零件…
navicust-sort-id = ID
navicust-sort-name = 名稱
navicust-sort-color = 顏色

# Patch card editor
patch-card-edit-search = 搜尋卡片…
patch-card-edit-count = { $count } 張卡片
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = 名稱
patch-card-sort-mb = MB
patch-card4-none = 無

# Auto Battle Data pane
auto-battle-data-secondary-standard-chips = 標準晶片（副）
auto-battle-data-standard-chips = 標準晶片
auto-battle-data-mega-chips = MEGA 晶片
auto-battle-data-giga-chip = GIGA 晶片
auto-battle-data-combos = 連段
auto-battle-data-program-advance = 程式進階

# Auto Battle Data editor
auto-battle-data-edit-used = 已使用
auto-battle-data-edit-secondary = 副
auto-battle-data-edit-count = { $count } 個晶片

# Common
save-empty = 此存檔沒有此檢視的資料。
play-no-selection = 選擇一個遊戲與存檔以檢視。

# Replays
replays-filter-all-games = 所有遊戲
replays-filter-opponent-placeholder = 搜尋對手…
replays-show-incomplete = 顯示未完成
replays-direct-marker = （直連）
replays-watch = 觀看
replays-watch-missing-rom = 觀看（尚未掃描此遊戲的 ROM）
replays-export = 算繪
replays-export-progress = 正在算繪…
replays-export-cancel = 取消
replays-export-cancelling = 正在取消…
replays-export-success = 算繪完成。
replays-export-error = 算繪失敗：{ $error }
replays-export-open = 開啟算繪檔案
replays-export-reset = 重設
replays-export-scale = 縮放
replays-export-scale-lossless = 無損
replays-export-disable-bgm = 靜音
replays-export-twosided = 雙方視角
replays-export-rounds = 回合：
replays-export-save-as = 另存新檔…
playback-close = 關閉
playback-play = 播放
playback-pause = 暫停
playback-options = 選項
playback-speed = 速度
playback-disconnect = 中斷連線
playback-disconnect-prompt = 要從此對戰中斷連線嗎？
playback-disconnect-detail = 你將結束與對手的對戰。
playback-cancel = 取消
playback-reconnecting = 連線已中斷
playback-reconnecting-detail = 正在重新連線…
replays-select-prompt = 選擇一個重播。
play-opponent = 對手
replays-match-type = 對戰類型：
replays-duration = 時長：
replays-round-count = { $count } 個回合
replays-incomplete = 未完成
play-you = 自己

# Patches
patches-update = 更新
patches-updating = 正在更新…
patches-update-failed = 更新失敗：{ $error }
patches-open-folder = 開啟資料夾
patches-favorite = 收藏
patches-unfavorite = 取消收藏
patches-search-placeholder = 搜尋補丁…
patches-select-prompt = 選擇一個補丁。
patches-readme-placeholder = 此補丁沒有 README。
patches-details-authors = 作者：
patches-details-license = 授權：
patches-details-source = 來源：
patches-details-games = 支援的遊戲：
patches-netplay-compatibility = 連線相容性：

# Settings panel
settings-section-general = 一般
settings-section-graphics = 圖形
settings-section-netplay = 連線對戰
settings-section-audio = 音訊
settings-volume = 音量
settings-disable-bgm-in-pvp = 連線對戰時靜音
settings-nickname = 暱稱
settings-language = 語言
settings-data-path = 資料路徑
settings-streamer-mode = 實況主隱私模式
settings-section-experimental = 實驗性功能
settings-enable-save-editor = 啟用存檔編輯器
settings-experimental-warning = 實驗性功能可能損壞或破壞你的存檔，可能隨時遭到變更或移除，並且可能缺少使存檔在連線對戰中保持合法的檢查。使用風險自負。
settings-section-about = 關於
settings-section-input = 輸入
settings-input-press-key = 按下按鍵或按鈕…
settings-input-add = 新增綁定
settings-input-reset = 還原預設
settings-input-select-hint = 點擊按鍵以編輯其綁定
input-key-up = 上
input-key-down = 下
input-key-left = 左
input-key-right = 右
input-key-a = A
input-key-b = B
input-key-l = L
input-key-r = R
input-key-start = Start
input-key-select = Select
input-key-speed-up = 快轉
input-gamepad-south = A 鍵
input-gamepad-east = B 鍵
input-gamepad-west = X 鍵
input-gamepad-north = Y 鍵
input-gamepad-select = Select
input-gamepad-start = Start
input-gamepad-mode = 導引鍵
input-gamepad-left-thumb = 左搖桿
input-gamepad-right-thumb = 右搖桿
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = 方向鍵 上
input-gamepad-dpad-down = 方向鍵 下
input-gamepad-dpad-left = 方向鍵 左
input-gamepad-dpad-right = 方向鍵 右
input-gamepad-misc1 = 其他 1
input-gamepad-misc2 = 其他 2
input-gamepad-misc3 = 其他 3
input-gamepad-misc4 = 其他 4
input-gamepad-misc5 = 其他 5
input-gamepad-misc6 = 其他 6
input-gamepad-right-paddle1 = 右背鍵 1
input-gamepad-left-paddle1 = 左背鍵 1
input-gamepad-right-paddle2 = 右背鍵 2
input-gamepad-left-paddle2 = 左背鍵 2
input-gamepad-touchpad = 觸控板
input-gamepad-axis-left-stick-x = 左搖桿 X
input-gamepad-axis-left-stick-y = 左搖桿 Y
input-gamepad-axis-right-stick-x = 右搖桿 X
input-gamepad-axis-right-stick-y = 右搖桿 Y
input-gamepad-axis-trigger-left = 左扳機
input-gamepad-axis-trigger-right = 右扳機
settings-theme = 主題
settings-theme-dark = 深色
settings-theme-light = 淺色
settings-matchmaking-endpoint = 配對伺服器位址
settings-data-folder = 資料夾
settings-data-folder-change = 變更…
settings-patch-repo = 補丁儲存庫
settings-enable-patch-autoupdate = 在背景自動更新補丁
settings-enable-updater = 自動檢查應用程式更新
settings-allow-prerelease-upgrades = 檢查應用程式更新時包含預先發行版本
settings-netplay-frame-delay = 影格延遲
settings-use-relay = 使用中繼伺服器
settings-use-relay-auto = 自動
settings-use-relay-always = 總是
settings-use-relay-never = 從不
settings-show-opponent-setup = 對戰開始時顯示對手的配置
settings-window-size = 視窗大小
settings-fullscreen = 全螢幕
settings-ui-scale = UI 縮放
settings-video-filter = 影片濾鏡
settings-fractional-scaling = 分數縮放
settings-hide-emulator-border = 隱藏模擬器邊框
updater-current-version = 目前版本：{ $version }
updater-latest-version = 最新版本：{ $version }
updater-loading = 檢查中…
updater-up-to-date = v{ $version }（最新版）
updater-downloading = 正在下載：{ $pct }%
updater-ready-to-update = 更新已下載，準備安裝。
updater-update-now = 立即更新

# Welcome screen
welcome-title = 歡迎使用 Tango！
welcome-subtitle = 開始遊玩前，你只需完成幾個步驟。
welcome-continue = 繼續
welcome-step-roms = 新增你的 ROM
welcome-step-roms-description = 將你的 Battle Network / Rockman EXE .gba 檔案放入：
welcome-step-roms-detected = 偵測到 { $count } 個 ROM。
welcome-step-nickname = 設定你的暱稱
welcome-step-nickname-description = 你可以隨時在設定中變更。
welcome-open-folder = 開啟 ROM 資料夾
welcome-roms-needed = 繼續前請至少新增一個 ROM。

# Common actions
rescan = 重新掃描

# Game names live in games.ftl — same Fluent attribute scheme the
# legacy app uses (game-<family> = base name; .variant-N for each
# regional/colour variant; .match-type-X-Y for per-mode labels).
