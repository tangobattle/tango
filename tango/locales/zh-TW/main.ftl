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
save-actions = 存檔操作

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
empty-scanning-title = 正在掃描遊戲庫…
empty-scanning-body = 正在讀取 ROM、存檔與補丁。

# Empty-state hints
empty-no-roms-title = 找不到遊戲 ROM
empty-no-roms-body = 將你的 Battle Network / Rockman EXE .gba 檔案放入：
empty-no-saves-title = 此遊戲沒有存檔檔案
empty-no-saves-body = 將此遊戲的 .sav 檔案放入：
play-patch-downloading = ↓ …
play-patch-downloading-progress = ↓ { $percent }%
play-patch-download-failed = ↓ 失敗
play-no-patch = 無補丁
play-patch-toggle = 使用補丁…
play-play = 開始遊戲
play-version-placeholder = —

# Play bottom strip
play-link-code = 連線代碼（留空則隨機產生）
play-link-code-random = 隨機連線代碼
play-training = 訓練
training-pip = 對手畫面
training-swap = 交換位置
training-chips = 強制晶片
training-chips-clear = 清空
training-chips-search = 搜尋晶片…
training-chips-unavailable = 此遊戲不支援強制晶片
training-dummy-auto-confirm = 假人晶片：自動確認
training-dummy-auto-possess = 假人晶片：由你挑選
training-dummy-manual = 假人晶片：手動
play-fight = 戰鬥
play-cancel = 離開
play-status-idle = 輸入連線代碼開始連線對戰，留空則進行單人遊戲。
play-status-connecting = 正在連線至配對伺服器…
play-status-direct-connecting = 正在連線至對手…
play-status-waiting-opponent = 正在等待對手…
play-status-negotiating = 正在協商…
play-status-failed = 連線失敗：{ $error }
play-status-peer-disconnected = 對方已離開。
play-status-signaling-version-too-old = 此版本的 Tango 過舊，無法進行連線對戰。請更新 Tango。
play-status-signaling-version-too-new = 配對伺服器版本過舊，不支援此版本的 Tango。
play-status-signaling-rejected = 配對伺服器拒絕了連線：{ $reason }
play-status-signaling-unreachable = 無法連線至配對伺服器：{ $error }
play-status-signaling-failed = 配對失敗：{ $error }
play-status-peer-connection-failed = 無法連線至對方：{ $error }
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
lobby-compat-fetching-patch = 正在下載本場對戰的補丁…
lobby-compat-fetching-patch-progress = 正在下載本場對戰的補丁… { $percent }%
lobby-compat-patch-failed = 無法下載本場對戰的補丁
lobby-compat-version-mismatch = 遊戲版本不一致（補丁 / ROM 不同）。
lobby-compat-sim-too-old = 本遊戲的連線對戰在對方的 Tango 版本之後有所變更 — 對方需要更新。
lobby-compat-sim-too-new = 本遊戲的連線對戰在你的 Tango 版本之後有所變更 — 你需要更新。
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
session-results-victory = 勝利！
session-results-defeat = 敗北
session-results-draw = 平手
session-results-no-contest = 對戰結束
session-results-disconnected = 對手已中斷連線
session-results-no-rounds = 對戰在分出任何回合勝負前就已結束。
session-results-vs = vs { $nickname }
session-results-you = 你
session-results-round = 第 { $number } 回合
session-results-draws = { $count } 個回合以平手作收
session-results-watch-replay = 觀看重播
session-results-done = 完成

# Save view sub-tabs

# Navi pane
navi-style = 樣式

# Folder pane
save-copy = 複製
copied = 已複製！

# Navi pane
navi-id = 領航員 ID
navi-link-navi = 連結領航員
navi-buster = 洛克砲
navi-power-attack = 強力攻擊
navi-style-unset = （無樣式）
navicust-parts = 已安裝的程式零件
navicust-empty = （未安裝）

# Navicust editor

# Patch card editor

# Auto Battle Data pane

# Auto Battle Data editor

# Common
save-empty = 此存檔沒有此檢視的資料。
play-no-selection = 選擇一個遊戲與存檔以檢視。

# Replays
replays-filter-all-games = 所有遊戲
replays-filter-any-time = 任何時間
replays-filter-past-day = 過去 24 小時
replays-filter-past-week = 過去一週
replays-filter-past-month = 過去一個月
replays-filter-past-year = 過去一年
replays-filter-search-placeholder = 搜尋重播…
replays-analyzing = 正在分析重播…
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
replays-export-setup = 準備
replays-export-rounds-analyzing = 回合：正在分析對戰…
replays-export-save-as = 另存新檔…
playback-close = 關閉
playback-play = 播放
playback-pause = 暫停
playback-options = 選項
playback-speed = 速度
playback-input-display = 輸入顯示
playback-pip = 對手畫面
playback-swap-perspective = 對手視角
playback-clip-tools = 剪輯
playback-clip-start = 標記剪輯起點
playback-clip-end = 標記剪輯終點
playback-clip-clear = 清除剪輯標記
playback-clip-export = 匯出剪輯
playback-disconnect = 中斷連線
playback-disconnect-prompt = 要從此對戰中斷連線嗎？
playback-disconnect-detail = 你將結束與對手的對戰。
playback-cancel = 取消
playback-reconnecting = 連線已中斷
playback-reconnecting-detail = 正在重新連線…
playback-exit-hold = 正在結束…
playback-exit-hold-detail = 按住 Esc 結束——放開取消。
playback-priming-match = 正在開始對戰…
playback-priming-match-detail = 兩邊的遊戲正在啟動到戰鬥畫面。
playback-priming-peer = 正在等待對手…
playback-priming-peer-detail = 對方的遊戲仍在啟動。
playback-priming-replay = 正在開始重播…
playback-priming-replay-detail = 遊戲正在啟動到戰鬥畫面。
playback-priming-elapsed = { $secs } 秒
playback-priming-failed = 遊戲未能進入戰鬥。
replays-select-prompt = 選擇一個重播。
replays-streamer-hidden = HP 與晶片記錄在實況模式下隱藏。
replays-streamer-show = 顯示
replays-queue-add = 加入佇列
replays-queue-count = 佇列中 { $n } 個
replays-queue-play = 播放佇列
replays-queue-clear = 清空佇列
replays-queue-remove = 從佇列移除
replays-queue-missing = 找不到重播檔案
replays-queue-up-next = 接下來 { $n } 個
replays-scanning = 正在掃描重播…
play-opponent = 對手
replays-match-type = 對戰類型：
replays-duration = 時長：
replays-round-count = { $count } 個回合
replays-incomplete = 未完成
play-you = 自己
patches-refresh = 重新整理
patches-refreshing = 正在重新整理…
patches-refresh-failed = 重新整理失敗：{ $error }
patches-install = 安裝
patches-uninstall = 解除安裝
patches-installed = 已安裝
patches-cancel = 取消下載
replays-patch-downloading = 正在下載此重播的補丁…
replays-patch-downloading-progress = 正在下載此重播的補丁… { $percent }%
replays-patch-download-failed = 無法下載此重播的補丁
patches-downloading = 正在下載…
patches-downloading-progress = 正在下載… { $percent }%
patches-download-failed = 下載失敗
patches-retry = 重試
patches-reveal-package = 顯示補丁包

# Patches
patches-open-folder = 開啟資料夾
patches-favorite = 收藏
patches-unfavorite = 取消收藏
patches-search-placeholder = 搜尋補丁…
patches-filter-all = 全部
patches-filter-installed = 已安裝
patches-filter-available = 可用
patches-select-prompt = 選擇一個補丁。
patches-scanning = 正在掃描補丁…
patches-readme-placeholder = 此補丁沒有 README。
patches-details-authors = 作者：
patches-details-license = 授權：
patches-details-source = 來源：
patches-details-games = 支援的遊戲：
patches-netplay-compatibility = 連線相容性：
patches-netplay-isolated = 僅限此版本
patches-netplay-vanilla = 可與無補丁的遊戲對戰
patches-netplay-group = 可與以下對戰：{ $group }

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
settings-streamer-mode = 實況模式
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
input-key-x = X
input-key-y = Y
input-key-l = L
input-key-r = R
input-key-start = Start
input-key-select = Select
input-key-mic = 對著麥克風吹氣
input-key-speed-up = 快轉
input-key-training-swap = 訓練：交換位置
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
settings-accent = 強調色
settings-accent-tango-green = 探戈綠
settings-accent-megaman-blue = 洛克人藍
settings-accent-protoman-red = 布魯斯紅
settings-accent-roll-pink = 羅爾粉紅
settings-accent-gutsman-yellow = 氣力人黃
settings-accent-bass-purple = 佛魯特紫
settings-group-profile = 個人資料
settings-group-interface = 介面
settings-group-storage = 儲存
settings-group-patches = 補丁
settings-group-updates = 更新
settings-group-window = 視窗
settings-group-emulator = 模擬器
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
settings-group-ds = 任天堂DS
settings-ds-screen-stacking = 螢幕排列
settings-ds-screen-stacking-horizontal = 水平
settings-ds-screen-stacking-vertical = 垂直
settings-ds-screen-stacking-primary-only = 僅主螢幕
settings-ds-primary-screen = 主螢幕
settings-ds-primary-screen-upper = 上螢幕
settings-ds-primary-screen-touch = 觸控螢幕
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
