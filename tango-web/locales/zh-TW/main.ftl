## tango-web strings. Keys shared with the desktop client are
## extracted from its locale of the same name; keep them in sync.

LANGUAGE = 繁體中文（台灣、香港、澳門）
window-quit = 結束 Tango
tab-play = 對戰
tab-replays = 重播
tab-patches = 補丁
tab-settings = 設定
play-no-game = 未選擇遊戲
play-no-save = 選擇存檔
play-no-patch = 無補丁
play-version-placeholder = —
play-link-code = 連線代碼（留空則隨機產生）
play-play = 開始
play-fight = 戰鬥
play-cancel = 離開
play-no-selection = 選擇一個遊戲與存檔以檢視。
play-status-connecting = 正在連線至配對伺服器…
play-status-waiting-opponent = 正在等待對手…
play-you = 自己
play-opponent = 對手
empty-no-roms-title = 找不到遊戲 ROM
empty-no-roms-body = 將你的 Battle Network / Rockman EXE .gba 檔案放入：
lobby-waiting = 等待中…
lobby-latency = 延遲：{ $ms } 毫秒
lobby-link-code = 連線代碼：{ $code }
lobby-match-type = 對戰類型
lobby-ready = 準備
lobby-unready = 取消準備
lobby-match-starting = 開始中…
lobby-compat-ok = 相容 — 可以開始對戰。
lobby-compat-missing-game = 有一方尚未選擇遊戲。
lobby-compat-missing-rom = 雙方並未都安裝該遊戲或補丁。
lobby-compat-match-mismatch = 對戰類型不一致。
lobby-pick-game-first = 請先選擇遊戲
lobby-no-match-types = （此遊戲沒有可用的對戰類型）
session-results-victory = 勝利！
session-results-defeat = 敗北
session-results-draw = 平手
session-results-no-contest = 對戰結束
session-results-vs = vs { $nickname }
session-results-you = 你
session-results-round = 第 { $number } 回合
session-results-draws = { $count } 個回合以平手作收
session-results-done = 完成
discord-presence-in-single-player = 單人遊戲中
discord-presence-in-progress = 對戰進行中
playback-pause = 暫停
playback-close = 關閉
replays-watch = 觀看
replays-incomplete = 未完成
replays-watch-missing-rom = 觀看（尚未掃描此遊戲的 ROM）
save-delete = 刪除
patches-update = 更新
patches-updating = 正在更新…
patches-update-failed = 更新失敗：{ $error }
patches-netplay-compatibility = 連線相容性：
patches-details-games = 支援的遊戲：
patches-select-prompt = 選擇一個補丁。
settings-patch-repo = 補丁儲存庫
settings-section-general = 一般
settings-section-graphics = 圖形
settings-section-audio = 音訊
settings-section-input = 輸入
settings-section-about = 關於
settings-volume = 音量
settings-nickname = 暱稱
settings-language = 語言
settings-input-press-key = 按下按鍵或按鈕…
settings-input-reset = 還原預設
settings-input-select-hint = 點擊按鍵以編輯其綁定
welcome-title = 歡迎使用 Tango！
welcome-subtitle = 開始遊玩前，你只需完成幾個步驟。
welcome-continue = 繼續
welcome-step-roms = 新增你的 ROM
welcome-step-roms-detected = 偵測到 { $count } 個 ROM。
welcome-step-nickname = 設定你的暱稱
welcome-roms-needed = 繼續前請至少新增一個 ROM。

## save view (extracted from the desktop's main.ftl; keep in sync)
save-tab-navicust = 領航客製
save-tab-folder = 卡組
save-tab-patch-cards = 改造卡
save-tab-auto-battle-data = 自動戰鬥資料
save-empty = 此存檔沒有此檢視的資料。
save-copy = 複製
save-copy-image = 複製為圖片
copied = 已複製！
save-edit = 編輯
save-edit-save = 儲存
save-edit-cancel = 取消
save-edit-sort = 排序
save-edit-clear = 全部清除
folder-group = 依晶片分組
navi-style = 樣式
navi-style-unset = （無樣式）
navi-id = 領航員 ID
navi-link-navi = 連結領航員
navi-base-hp = HP
navi-buster = 洛克砲
navi-buster-attack = 攻擊
navi-buster-rapid = 連射
navi-buster-charge = 蓄力
navi-power-attack = 強力攻擊
navi-edit-select = 領航員
folder-edit-search = 搜尋晶片…
folder-edit-folder = 卡組
folder-edit-count = { $count } / { $limit }
folder-edit-navi = 領航員 { $used } / { $limit }
folder-edit-mega = MEGA { $used } / { $limit }
folder-edit-giga = GIGA { $used } / { $limit }
folder-edit-dark = 黑暗 { $used } / { $limit }
folder-edit-reg-memory = 常規 { $mb }MB
folder-edit-tag-memory = 搭檔 { $mb }MB
folder-sort-id = ID
folder-sort-name = 名稱
folder-sort-code = 代碼
folder-sort-attack = 攻擊力
folder-sort-element = 屬性
folder-sort-mb = MB
navicust-grid-size = 格線：{ $cols } × { $rows }
navicust-parts = 已安裝的程式零件
navicust-empty = （未安裝）
navicust-edit-grid = 領航客製
navicust-edit-count = { $count } 個程式零件
navicust-edit-rotate = 旋轉
navicust-edit-compress = 壓縮
navicust-edit-uncompress = 解壓縮
navicust-edit-search = 搜尋程式零件…
navicust-sort-id = ID
navicust-sort-name = 名稱
navicust-sort-color = 顏色
patch-card-edit-search = 搜尋卡片…
patch-card-edit-count = { $count } 張卡片
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = 名稱
patch-card-sort-mb = MB
patch-card4-none = 無
auto-battle-data-secondary-standard-chips = 標準晶片（副）
auto-battle-data-standard-chips = 標準晶片
auto-battle-data-mega-chips = MEGA 晶片
auto-battle-data-giga-chip = GIGA 晶片
auto-battle-data-combos = 連段
auto-battle-data-program-advance = 程式進階
auto-battle-data-edit-used = 已使用
auto-battle-data-edit-secondary = 副
auto-battle-data-edit-count = { $count } 個晶片
save-actions = 存檔操作
save-duplicate = 建立副本
save-rename = 重新命名
save-rename-confirm = 重新命名
save-delete-confirm = 刪除
save-action-cancel = 取消
save-delete-prompt = 刪除 { $name }？
save-name-placeholder = 新名稱
save-new = 新增存檔
save-new-confirm = 建立
save-template-default = （預設）
save-template-pick = 選擇範本…

## replays detail (extracted from the desktop's main.ftl; keep in sync)
replays-filter-all-games = 所有遊戲
replays-filter-any-time = 任何時間
replays-filter-past-day = 過去 24 小時
replays-filter-past-week = 過去一週
replays-filter-past-month = 過去一個月
replays-filter-past-year = 過去一年
replays-filter-search-placeholder = 搜尋重播…
replays-show-incomplete = 顯示未完成
replays-direct-marker = （直連）
replays-select-prompt = 選擇一個重播。
replays-match-type = 對戰類型：
replays-duration = 時長：
replays-round-count = { $count } 個回合

## patches detail (extracted from the desktop's main.ftl; keep in sync)
patches-favorite = 收藏
patches-unfavorite = 取消收藏
patches-search-placeholder = 搜尋補丁…
patches-readme-placeholder = 此補丁沒有 README。
patches-details-authors = 作者：
patches-details-license = 授權：
patches-details-source = 來源：

## netplay settings (extracted from the desktop's main.ftl; keep in sync)
settings-matchmaking-endpoint = 配對伺服器位址
settings-use-relay = 使用中繼伺服器
settings-use-relay-auto = 自動
settings-use-relay-always = 總是
settings-use-relay-never = 從不
settings-show-opponent-setup = 對戰開始時顯示對手的配置

## netplay settings section label (extracted from the desktop)
settings-section-netplay = 連線對戰

## accent + patch repo settings (extracted from the desktop)
settings-accent = 強調色
settings-accent-tango-green = 探戈綠
settings-accent-megaman-blue = 洛克人藍
settings-accent-protoman-red = 布魯斯紅
settings-accent-roll-pink = 羅爾粉紅
settings-accent-gutsman-yellow = 氣力人黃
settings-accent-bass-purple = 佛魯特紫

## welcome step description (extracted from the desktop)
welcome-step-nickname-description = 你可以隨時在設定中變更。

## replay video export (extracted from the desktop)
replays-export = 算繪
replays-export-progress = 正在算繪…
replays-export-cancel = 取消
replays-export-success = 算繪完成。
replays-export-error = 算繪失敗：{ $error }

## theme + streamer + autoupdate settings (extracted from the desktop)
settings-theme = 主題
settings-theme-dark = 深色
settings-theme-light = 淺色
settings-streamer-mode = 實況主隱私模式
settings-enable-patch-autoupdate = 在背景自動更新補丁

## mute bgm setting (extracted from the desktop)
settings-disable-bgm-in-pvp = 連線對戰時靜音

## cover tab (extracted from the desktop)
save-tab-cover = 封面

## lobby + telemetry + transport (extracted from the desktop)
lobby-blind-mine = 隱藏配置
lobby-blind-self-on = 你正在隱藏自己的配置。
lobby-blind-peer-on = 對手正在隱藏其配置。
settings-netplay-frame-delay = 影格延遲
lobby-frame-delay-suggest = 根據延遲建議
session-stat-tps = 每秒影格數（目前/最大）
session-stat-skew = 時鐘偏移
session-stat-lead = 領先
session-stat-depth = 預測錯誤深度
session-stat-ping = 網路延遲
playback-speed = 速度
playback-input-display = 輸入顯示

## replay input display + swap (extracted from the desktop)
playback-swap-perspective = 對手視角
