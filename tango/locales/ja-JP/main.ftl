# Window
# Endonym for this locale; shown in the language picker.
LANGUAGE = 日本語

window-title = Tango
# Tooltip on the top bar's close button (fullscreen only).
window-quit = Tango を終了

# Crash handler dialogs (parent process)
crash = Tango がエラーで終了しました。

    報告の際は次のログファイルを添付してください：

    { $path }
crash-no-log = Tango がエラーで終了しました。

    { $error }

# Discord rich presence
discord-presence-looking = 募集中
discord-presence-in-single-player = 一人用モード中
discord-presence-in-lobby = ロビー待機中
discord-presence-in-progress = 対戦中

# Top-bar tabs
tab-play = 対戦
tab-replays = リプレイ
tab-patches = パッチ
tab-settings = 設定

# Play selectors
play-no-game = ゲーム未選択
play-no-save = セーブを選択
save-actions = セーブデータの操作

# Save management
save-open-folder = フォルダを開く
save-duplicate = 複製
save-rename = 名前変更
save-delete = 削除
save-rename-confirm = 名前変更
save-delete-confirm = 削除
save-action-cancel = キャンセル
save-delete-prompt = { $name } を削除しますか？
save-name-placeholder = 新しい名前
save-new = 新規セーブ
save-new-confirm = 作成
save-template-default = （デフォルト）
save-template-pick = テンプレートを選択…
empty-scanning-title = ライブラリをスキャンしています…
empty-scanning-body = ROM・セーブ・パッチを読み込んでいます。

# Empty-state hints
empty-no-roms-title = ROMが見つかりません
empty-no-roms-body = バトルネットワーク／ロックマンエグゼの .gba ファイルを次の場所に置いてください：
empty-no-saves-title = このゲームのセーブがありません
empty-no-saves-body = このゲームの .sav ファイルを次の場所に置いてください：
play-patch-downloading = ↓ …
play-patch-downloading-progress = ↓ { $percent }%
play-patch-download-failed = ↓ 失敗
play-no-patch = パッチなし
play-patch-toggle = パッチを使用…
play-play = プレイ
play-version-placeholder = —

# Play bottom strip
play-link-code = リンクコード（空欄でランダム生成）
play-link-code-random = ランダムリンクコード
play-training = トレーニング
training-pip = 相手の画面
training-swap = 左右を入れ替える
play-fight = 対戦
play-cancel = キャンセル
play-status-idle = ネットプレイするにはリンクコードを入力、空欄で一人用モードになります。
play-status-connecting = マッチメイキングサーバーに接続中…
play-status-direct-connecting = 対戦相手に接続中…
play-status-waiting-opponent = 対戦相手を待っています…
play-status-negotiating = ネゴシエーション中…
play-status-failed = 接続失敗: { $error }
play-status-peer-disconnected = 相手が退出しました。
play-status-signaling-version-too-old = このバージョンのTangoはオンライン対戦には古すぎます。Tangoを更新してください。
play-status-signaling-version-too-new = マッチメイキングサーバーがこのバージョンのTangoに対応していません。
play-status-signaling-rejected = マッチメイキングサーバーに接続を拒否されました: { $reason }
play-status-signaling-unreachable = マッチメイキングサーバーに接続できませんでした: { $error }
play-status-signaling-failed = マッチメイキングに失敗しました: { $error }
play-status-peer-connection-failed = 相手に接続できませんでした: { $error }
play-status-negotiate-expected-hello = 相手から想定したハンドシェイクが届きませんでした。
play-status-negotiate-version-too-old = 相手は古いバージョンのTangoを使用しています。
play-status-negotiate-version-too-new = 相手は新しいバージョンのTangoを使用しています。
play-status-negotiate-failed = ネゴシエーション中にエラーが発生しました: { $error }
lobby-waiting = 待機中…
lobby-no-game = （ゲーム未選択）
lobby-latency = Ping: { $ms } ms
lobby-latency-direct = Ping（直接）: { $ms } ms
lobby-latency-relayed = Ping（中継）: { $ms } ms
lobby-link-code = リンクコード: { $code }
lobby-direct-host = UDP ポート { $port } でホスト中
lobby-direct-connect = UDP で { $target } に接続中
lobby-handshake = 設定交換中…
lobby-match-type = マッチタイプ
settings-netplay-frame-delay = フレーム遅延
settings-use-relay = リレーサーバーを使用
settings-use-relay-auto = 自動
settings-use-relay-always = 常に使用
settings-use-relay-never = 使用しない
settings-show-opponent-setup = 対戦開始時に相手の構築を表示
lobby-frame-delay-suggest = Pingから推奨
lobby-no-match-types = （このゲームには対戦モードがありません）
lobby-pick-game-first = まずゲームを選んでください
lobby-compat-ok = 互換あり — 対戦できます。
lobby-compat-missing-game = ゲームが選択されていない側があります。
lobby-compat-missing-rom = どちらかにゲームまたはパッチがインストールされていません。
lobby-compat-fetching-patch = この対戦のパッチをダウンロードしています…
lobby-compat-fetching-patch-progress = この対戦のパッチをダウンロードしています… { $percent }%
lobby-compat-patch-failed = この対戦のパッチをダウンロードできませんでした
lobby-compat-version-mismatch = ゲームのバージョンが一致しません（パッチ／ROM が異なる）。
lobby-compat-sim-too-old = このゲームのネットプレイは相手のTangoのバージョン以降に変更されました — 相手の更新が必要です。
lobby-compat-sim-too-new = このゲームのネットプレイはあなたのTangoのバージョン以降に変更されました — 更新が必要です。
lobby-compat-match-mismatch = 対戦モードが一致しません。
lobby-ready = 準備完了
lobby-unready = 取消
lobby-match-starting = 開始中…
lobby-blind-mine = 構築を隠す
lobby-blind-peer-on = 相手は構築を隠しています。
lobby-blind-self-on = 自分の構築を隠しています。
session-opponent = 相手の構築
session-self = 自分の構築
session-back-to-session = 対戦に戻る
# PvP telemetry deck cell tooltips
session-stat-tps = 毎秒ティック数（現在/上限）
session-stat-skew = ずれ
session-stat-depth = 予測ミス深度
session-stat-ping = ネットワーク遅延
session-results-victory = 勝利！
session-results-defeat = 敗北
session-results-draw = 引き分け
session-results-no-contest = 対戦終了
session-results-disconnected = 相手の接続が切断されました
session-results-no-rounds = ラウンドの決着がつく前に対戦が終了しました。
session-results-vs = vs { $nickname }
session-results-you = あなた
session-results-round = ラウンド{ $number }
session-results-draws = { $count }ラウンドが引き分けに終わりました
session-results-watch-replay = リプレイを再生
session-results-done = 完了

# Save view sub-tabs

# Navi pane
navi-style = スタイル

# Folder pane
save-copy = コピー
copied = コピーしました！

# Navi pane
navi-id = ナビID
navi-link-navi = リンクナビ
navi-buster = バスター
navi-power-attack = パワーアタック
navi-style-unset = （スタイルなし）
navicust-parts = 設定済みパーツ
navicust-empty = （未設定）

# Folder editor

# Navicust editor

# Patch card editor

# Auto Battle Data pane

# Auto Battle Data editor

# Common
save-empty = このセーブにはこのビューのデータがありません。
play-no-selection = 検査するゲームとセーブを選択してください。

# Replays
replays-filter-all-games = すべて
replays-filter-any-time = すべての期間
replays-filter-past-day = 過去24時間
replays-filter-past-week = 過去1週間
replays-filter-past-month = 過去1か月
replays-filter-past-year = 過去1年
replays-filter-search-placeholder = リプレイを検索…
replays-analyzing = リプレイを解析中…
replays-show-incomplete = 未完了も表示
replays-direct-marker = （ダイレクト）
replays-watch = 再生
replays-watch-missing-rom = 再生（このゲームのROMが未スキャン）
replays-export = 録画
replays-export-progress = 録画中…
replays-export-cancel = キャンセル
replays-export-cancelling = キャンセル中…
replays-export-success = 録画が完了しました。
replays-export-error = 録画に失敗しました: { $error }
replays-export-open = 動画を開く
replays-export-reset = リセット
replays-export-scale = 拡大率
replays-export-scale-lossless = ロスレス
replays-export-disable-bgm = 音楽を消す
replays-export-twosided = 両面表示
replays-export-rounds = ラウンド:
replays-export-rounds-analyzing = ラウンド: 対戦を解析中…
replays-export-save-as = 名前を付けて保存…
playback-close = 閉じる
playback-options = オプション
playback-speed = 速度
playback-input-display = 入力表示
playback-pip = 相手の画面
playback-swap-perspective = 相手の視点
playback-clip-tools = クリップ
playback-clip-start = クリップの開始位置を設定
playback-clip-end = クリップの終了位置を設定
playback-clip-clear = クリップ範囲をクリア
playback-clip-export = クリップを書き出す
playback-play = 再生
playback-pause = 一時停止
playback-disconnect = 切断
playback-disconnect-prompt = この試合から切断しますか？
playback-disconnect-detail = 相手との試合を終了します。
playback-cancel = キャンセル
replays-select-prompt = リプレイを選択してください。
replays-streamer-hidden = HPとチップの履歴は配信モードでは非表示です。
replays-streamer-show = 表示
replays-queue-add = キューに追加
replays-queue-count = { $n }件キュー中
replays-queue-play = キューを再生
replays-queue-clear = キューを空にする
replays-queue-remove = キューから削除
replays-queue-missing = リプレイファイルがありません
replays-queue-up-next = 次に{ $n }件
replays-scanning = リプレイをスキャンしています…
play-opponent = 相手
replays-match-type = マッチタイプ:
replays-duration = 再生時間:
replays-round-count = { $count }ラウンド
replays-incomplete = 未完了
play-you = 自分
patches-refresh = 更新
patches-refreshing = 更新中…
patches-refresh-failed = 更新に失敗しました: { $error }
patches-install = インストール
patches-uninstall = 削除
patches-installed = インストール済み
patches-cancel = ダウンロードをキャンセル
replays-patch-downloading = このリプレイのパッチをダウンロードしています…
replays-patch-downloading-progress = このリプレイのパッチをダウンロードしています… { $percent }%
replays-patch-download-failed = このリプレイのパッチをダウンロードできませんでした
patches-downloading = ダウンロード中…
patches-downloading-progress = ダウンロード中… { $percent }%
patches-download-failed = ダウンロードに失敗しました
patches-retry = 再試行
patches-reveal-package = パッケージを表示

# Patches
patches-update = 更新
patches-updating = 更新中…
patches-update-failed = 更新に失敗しました: { $error }
patches-open-folder = フォルダを開く
patches-favorite = お気に入り
patches-unfavorite = お気に入り解除
patches-search-placeholder = パッチを検索…
patches-filter-all = すべて
patches-filter-installed = インストール済み
patches-filter-available = 利用可能
patches-select-prompt = パッチを選択してください。
patches-scanning = パッチをスキャンしています…
patches-readme-placeholder = このパッチにはREADMEがありません。
patches-details-authors = 作者:
patches-details-license = ライセンス:
patches-details-source = ソース:
patches-details-games = 対応ゲーム:
patches-netplay-compatibility = ネットプレイ互換性:
patches-netplay-isolated = このバージョンのみ
patches-netplay-vanilla = 未パッチのゲームと対戦可能
patches-netplay-group = 対戦可能: { $group }

# Settings panel
settings-section-general = 一般
settings-section-graphics = グラフィック
settings-section-netplay = ネットプレイ
settings-section-audio = オーディオ
settings-volume = 音量
settings-disable-bgm-in-pvp = ネットプレイで音楽を消す
settings-nickname = ニックネーム
settings-language = 言語
settings-data-path = データパス
settings-streamer-mode = 配信モード
settings-section-experimental = 実験的機能
settings-enable-save-editor = セーブエディターを有効にする
settings-experimental-warning = 実験的機能はセーブデータを破損させたり使用できなくする可能性があり、予告なく変更・削除されることがあります。また、オンライン対戦でセーブを正規の状態に保つためのチェックが省かれている場合があります。自己責任でご利用ください。
settings-section-about = アプリ情報
settings-section-input = 入力
settings-input-press-key = キーまたはボタンを押してください…
settings-input-add = 割り当てを追加
settings-input-reset = 初期設定に戻す
settings-input-select-hint = ボタンをクリックして割り当てを編集
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
input-key-start = スタート
input-key-select = セレクト
input-key-mic = マイクに息をふきかける
input-key-speed-up = 早送り
input-gamepad-south = Aボタン
input-gamepad-east = Bボタン
input-gamepad-west = Xボタン
input-gamepad-north = Yボタン
input-gamepad-select = セレクト
input-gamepad-start = スタート
input-gamepad-mode = ガイド
input-gamepad-left-thumb = 左スティック
input-gamepad-right-thumb = 右スティック
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = 十字キー 上
input-gamepad-dpad-down = 十字キー 下
input-gamepad-dpad-left = 十字キー 左
input-gamepad-dpad-right = 十字キー 右
input-gamepad-misc1 = その他 1
input-gamepad-misc2 = その他 2
input-gamepad-misc3 = その他 3
input-gamepad-misc4 = その他 4
input-gamepad-misc5 = その他 5
input-gamepad-misc6 = その他 6
input-gamepad-right-paddle1 = 右パドル 1
input-gamepad-left-paddle1 = 左パドル 1
input-gamepad-right-paddle2 = 右パドル 2
input-gamepad-left-paddle2 = 左パドル 2
input-gamepad-touchpad = タッチパッド
input-gamepad-axis-left-stick-x = 左スティック X
input-gamepad-axis-left-stick-y = 左スティック Y
input-gamepad-axis-right-stick-x = 右スティック X
input-gamepad-axis-right-stick-y = 右スティック Y
input-gamepad-axis-trigger-left = 左トリガー
input-gamepad-axis-trigger-right = 右トリガー
settings-theme = テーマ
settings-theme-dark = ダーク
settings-theme-light = ライト
settings-accent = アクセントカラー
settings-accent-tango-green = タンゴグリーン
settings-accent-megaman-blue = ロックマンブルー
settings-accent-protoman-red = ブルースレッド
settings-accent-roll-pink = ロールピンク
settings-accent-gutsman-yellow = ガッツマンイエロー
settings-accent-bass-purple = フォルテパープル
settings-group-profile = プロフィール
settings-group-interface = インターフェース
settings-group-storage = 保存先
settings-group-patches = パッチ
settings-group-updates = アップデート
settings-group-window = ウィンドウ
settings-group-emulator = エミュレーター
settings-matchmaking-endpoint = マッチメイキングエンドポイント
settings-patch-repo = パッチリポジトリ
settings-enable-patch-autoupdate = パッチを自動更新する
settings-enable-updater = アプリの更新を自動チェック
settings-allow-prerelease-upgrades = プレリリースも対象にする
settings-window-size = ウィンドウサイズ
settings-fullscreen = フルスクリーン
settings-ui-scale = UI拡大率
settings-fractional-scaling = フラクショナルスケーリング
settings-group-ds = ニンテンドーDS
settings-ds-screen-stacking = 画面の配置
settings-ds-screen-stacking-horizontal = 横並び
settings-ds-screen-stacking-vertical = 縦並び
settings-ds-screen-stacking-primary-only = メイン画面のみ
settings-ds-primary-screen = メイン画面
settings-ds-primary-screen-upper = 上画面
settings-ds-primary-screen-touch = タッチ画面
settings-hide-emulator-border = エミュレーターの枠を非表示
settings-video-filter = ビデオフィルター
updater-current-version = 現在のバージョン: { $version }
updater-latest-version = 最新バージョン: { $version }
updater-loading = 確認中…
updater-up-to-date = v{ $version } (最新)
updater-downloading = ダウンロード中: { $pct }%
updater-ready-to-update = 更新の準備が完了しました。
updater-update-now = 今すぐ更新

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

# Reconnect / data folder / save-view tabs
session-stat-lead = リード
playback-reconnecting = 接続が切れました
playback-reconnecting-detail = 再接続中…
playback-exit-hold = 終了中…
playback-exit-hold-detail = Escを押し続けると終了します。離すとキャンセルされます。
playback-priming-match = 試合を開始中…
playback-priming-match-detail = 両方のゲームをバトルまで起動しています。
playback-priming-peer = 相手を待っています…
playback-priming-peer-detail = 相手のゲームはまだ起動中です。
playback-priming-replay = リプレイを開始中…
playback-priming-replay-detail = ゲームをバトルまで起動しています。
playback-priming-elapsed = { $secs }秒
playback-priming-failed = ゲームがバトルまで到達しませんでした。
settings-data-folder = データフォルダ
settings-data-folder-change = 変更…
