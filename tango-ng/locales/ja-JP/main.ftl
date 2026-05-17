# Window
window-title = Tango

# Top-bar tabs
tab-play = 対戦
tab-replays = リプレイ
tab-patches = パッチ
tab-settings = 設定

# Play selectors
play-no-game = ゲーム未選択
play-no-save = セーブを選択

# Save management
save-open-folder = フォルダを開く
save-duplicate = 複製
save-rename = 名前変更
save-delete = 削除
save-rename-confirm = 保存
save-delete-confirm = 削除
save-action-cancel = キャンセル
save-delete-prompt = このセーブを削除しますか？
save-name-placeholder = 新しい名前
save-new = 新規セーブ
save-new-confirm = 作成
save-template-default = （デフォルト）

# Empty-state hints
empty-no-roms-title = ROMが見つかりません
empty-no-roms-body = バトルネットワーク／ロックマンエグゼの .gba ファイルを次の場所に置いてください：
empty-no-saves-title = このゲームのセーブがありません
empty-no-saves-body = このゲームの .sav ファイルを次の場所に置いてください：
play-no-patch = パッチなし
play-version = バージョン
play-version-placeholder = —

# Play bottom strip
play-link-code = リンクコード
play-play = 対戦
play-cancel = キャンセル
play-status-idle = ネットプレイするにはリンクコードを入力、空欄で一人用モードになります。
play-status-connecting = 接続中:
play-status-connected = 接続済み:
play-status-failed = 接続失敗
play-netplay-todo = ネットプレイは未実装です。リンクコードを空にすると一人用セッションを開始できます。

# Save view sub-tabs
save-tab-cover = カバー
save-tab-navi = ナビ
save-tab-folder = フォルダ
save-tab-patch-cards = パッチカード
save-tab-auto-battle-data = オートバトルデータ
save-cover-description = このタブは意図的に空白にしてあります。

# Navi pane
navi-style = スタイル
navi-level = レベル
navi-stat-hp = HP
navi-stat-attack = アタック
navi-stat-rapid = ラピッド
navi-stat-charge = チャージ
navi-modcards = モッドカード

# Folder pane
folder-col-count = 枚数
folder-col-code = コード
folder-col-chip = チップ
folder-col-element = 属性
folder-col-power = 威力
folder-regular-chip = レギュラー
folder-tag-chips = タグ
folder-group = チップでまとめる
save-copy = コピー

# Navi pane
navi-id = ナビID
navi-style-unset = （スタイルなし）
navicust-grid-size = グリッド
navicust-parts = 設定済みパーツ
navicust-empty = （未設定）

# Patch cards pane
patch-cards-count = 装備数
patch-cards-4-title = パッチカード(4)

# Auto Battle Data pane
auto-battle-data-secondary-standard-chips = スタンダードチップ（補助）
auto-battle-data-standard-chips = スタンダードチップ
auto-battle-data-mega-chips = メガチップ
auto-battle-data-giga-chip = ギガチップ
auto-battle-data-combos = コンボ
auto-battle-data-program-advance = プログラムアドバンス

# Common
save-empty = このセーブにはこのビューのデータがありません。
play-no-selection = 検査するゲームとセーブを選択してください。

# Patch Cards / Auto Battle Data placeholders (legacy)
placeholder-patch-cards = パッチカードのグリッドがここに表示されます。
placeholder-auto-battle-data = オートバトルデータの表がここに表示されます。

# Replays
replays-folder-label = フォルダ
replays-watch = 再生
replays-export = エクスポート
playback-close = 閉じる
playback-failed = リプレイを再生できませんでした
playback-play = 再生
playback-pause = 一時停止
replays-all-replays = すべてのリプレイ
replays-select-prompt = リプレイを選択してください。
replays-preview-placeholder = セーブ／フォルダ／チップのプレビューがここに表示されます。
replays-round = ラウンド
replays-opponent = 相手
replays-match-type = マッチタイプ
play-you = 自分

# Patches
patches-update = 更新
patches-updating = 更新中…
patches-update-failed = 更新に失敗しました
patches-open-folder = フォルダを開く
patches-repo-label = リポジトリ
patches-installed = インストール済み
patches-select-prompt = パッチを選択してください。
patches-readme = README
patches-readme-placeholder = このパッチにはREADMEがありません。
patches-details-authors = 作者
patches-details-license = ライセンス
patches-details-source = ソース
patches-details-games = 対応ゲーム
patches-netplay-compatibility = ネットプレイ互換性

# Settings panel
settings-section-general = 一般
settings-section-graphics = グラフィック
settings-section-audio = オーディオ
settings-volume = 音量
settings-section-netplay = ネットプレイ
settings-nickname = ニックネーム
settings-language = 言語
settings-renderer = レンダラー
settings-scale = 拡大率
settings-audio-backend = バックエンド
settings-signaling = シグナリング
settings-data-path = データパス
settings-streamer-mode = 配信プライバシーモード
settings-section-about = アプリ情報
settings-theme = テーマ
settings-matchmaking-endpoint = マッチメイキングエンドポイント
settings-patch-repo = パッチリポジトリ
settings-version = バージョン
settings-about-blurb = tango-ng — Tango UI の iced 試作版。機能はまだ揃っていません。

# Welcome screen
welcome-title = Tango へようこそ！
welcome-subtitle = 対戦できる前にいくつかの初期設定をしてください。
welcome-continue = 続ける
welcome-step-roms = ROM を追加
welcome-step-roms-description = ロックマンエグゼ／Battle Network の .gba ファイルを次の場所に置いてください：
welcome-step-roms-detected = { $count } 個の ROM を検出しました。
welcome-step-nickname = ニックネームを設定
welcome-step-nickname-description = 設定からいつでも変更できます。
welcome-open-folder = ROM フォルダを開く
welcome-roms-needed = 続行するには ROM を 1 つ以上追加してください。

# Common actions
rescan = 再スキャン

# Game names live in games.ftl (Fluent attribute scheme shared with
# the legacy app: game-<family>.variant-N, .short, .match-type-X-Y).
