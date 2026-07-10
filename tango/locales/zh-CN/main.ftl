# Window
# Endonym for this locale; shown in the language picker.
LANGUAGE = 简体中文（中国）

window-title = Tango
# Tooltip on the top bar's close button (fullscreen only).
window-quit = 退出 Tango

# Crash handler dialogs (parent process)
crash = 糟糕，Tango 遇到错误并已崩溃！

    报告此崩溃时，请附上以下日志文件：

    { $path }
crash-no-log = 糟糕，Tango 遇到错误并已崩溃！

    { $error }

# Discord rich presence
discord-presence-looking = 正在寻找对战
discord-presence-in-single-player = 单人游戏中
discord-presence-in-lobby = 大厅中
discord-presence-in-progress = 对战进行中

# Top-bar tabs
tab-play = 对战
tab-replays = 录像
tab-patches = 补丁
tab-settings = 设置

# Play selectors
play-no-game = 未选择游戏
play-no-save = 选择存档

# Save management
save-open-folder = 打开文件夹
save-duplicate = 创建副本
save-rename = 重命名
save-delete = 删除
save-rename-confirm = 重命名
save-delete-confirm = 删除
save-action-cancel = 取消
save-delete-prompt = 删除 { $name }？
save-name-placeholder = 新名称
save-new = 新建存档
save-new-confirm = 创建
save-template-default = （默认）
save-template-pick = 选择模板…

# Empty-state hints
empty-no-roms-title = 未找到游戏 ROM
empty-no-roms-body = 将你的 Battle Network / Rockman EXE .gba 文件放入：
empty-no-saves-title = 此游戏没有存档文件
empty-no-saves-body = 将此游戏的 .sav 文件放入：
play-no-patch = 无补丁
play-patch-toggle = 使用补丁…
play-version-placeholder = —

# Play bottom strip
play-link-code = 链接代码（留空则随机生成）
play-link-code-random = 随机链接代码
play-play = 开始
play-fight = 战斗
play-cancel = 离开
play-status-idle = 输入链接代码开始联机对战，留空则进行单人游戏。
play-status-connecting = 正在连接匹配服务器…
play-status-direct-connecting = 正在连接对手…
play-status-waiting-opponent = 正在等待对手…
play-status-negotiating = 正在协商…
play-status-failed = 连接失败：{ $error }
play-status-peer-disconnected = 对方已离开。
play-status-negotiate-expected-hello = 对方未发送预期的握手信息。
play-status-negotiate-version-too-old = 对方运行的是较旧版本的 Tango。
play-status-negotiate-version-too-new = 对方运行的是较新版本的 Tango。
play-status-negotiate-failed = 协商过程中发生错误：{ $error }
lobby-waiting = 等待中…
lobby-no-game = （未选择游戏）
lobby-latency = 延迟：{ $ms } 毫秒
lobby-latency-direct = 延迟（直连）：{ $ms } 毫秒
lobby-latency-relayed = 延迟（中继）：{ $ms } 毫秒
lobby-link-code = 链接代码：{ $code }
lobby-direct-host = 正在 UDP 端口监听：{ $port }
lobby-direct-connect = 正在通过 UDP 连接：{ $target }
lobby-handshake = 正在交换设置…
lobby-match-type = 对战类型
lobby-frame-delay-suggest = 根据延迟推荐
lobby-no-match-types = （此游戏没有可用的对战类型）
lobby-pick-game-first = 请先选择游戏

lobby-compat-ok = 兼容 — 可以开始对战。
lobby-compat-missing-game = 有一方尚未选择游戏。
lobby-compat-missing-rom = 双方未都安装该游戏或补丁。
lobby-compat-version-mismatch = 游戏版本不一致（补丁 / ROM 不同）。
lobby-compat-match-mismatch = 对战类型不一致。
lobby-ready = 准备
lobby-unready = 取消准备
lobby-match-starting = 开始中…
lobby-blind-mine = 隐藏配置
lobby-blind-peer-on = 对手正在隐藏其配置。
lobby-blind-self-on = 你正在隐藏自己的配置。
session-opponent = 对手配置
session-self = 我的配置
session-back-to-session = 返回对战
# PvP telemetry deck cell tooltips
session-stat-tps = 每秒帧数（当前/最大）
session-stat-skew = 时钟偏移
session-stat-lead = 领先
session-stat-depth = 预测错误深度
session-stat-ping = 网络延迟

# Save view sub-tabs
save-tab-cover = 封面
save-tab-navi = 领航员
save-tab-navicust = 领航定制
save-tab-folder = 卡组
save-tab-patch-cards = 改造卡
save-tab-auto-battle-data = 自动战斗数据

# Navi pane
navi-style = 样式

# Folder pane
folder-group = 按芯片分组
save-copy = 复制
copied = 已复制！
save-copy-image = 复制为图片
save-edit = 编辑
save-edit-save = 保存
save-edit-cancel = 取消
folder-edit-search = 搜索芯片…
folder-edit-folder = 卡组
folder-edit-count = { $count } / { $limit }
folder-edit-navi = 领航员 { $used } / { $limit }
folder-edit-mega = MEGA { $used } / { $limit }
folder-edit-giga = GIGA { $used } / { $limit }
folder-edit-dark = 黑暗 { $used } / { $limit }
folder-edit-reg-memory = 常规 { $mb }MB
folder-edit-tag-memory = 搭档 { $mb }MB
save-edit-sort = 排序
save-edit-clear = 全部清除
folder-sort-id = ID
folder-sort-name = 名称
folder-sort-code = 代码
folder-sort-attack = 攻击力
folder-sort-element = 属性
folder-sort-mb = MB

# Navi pane
navi-id = 领航员 ID
navi-link-navi = 链接领航员
navi-edit-select = 领航员
navi-style-unset = （无样式）
navicust-grid-size = 网格：{ $cols } × { $rows }
navicust-parts = 已安装的程序零件
navicust-empty = （未安装）

# Navicust editor
navicust-edit-grid = 领航定制
navicust-edit-count = { $count } 个程序零件
navicust-edit-rotate = 旋转
navicust-edit-compress = 压缩
navicust-edit-uncompress = 解压
navicust-edit-search = 搜索程序零件…
navicust-sort-id = ID
navicust-sort-name = 名称
navicust-sort-color = 颜色

# Patch card editor
patch-card-edit-search = 搜索卡片…
patch-card-edit-count = { $count } 张卡片
patch-card-edit-mb = { $mb }MB / { $limit }MB
patch-card-sort-id = ID
patch-card-sort-name = 名称
patch-card-sort-mb = MB
patch-card4-none = 无

# Auto Battle Data pane
auto-battle-data-secondary-standard-chips = 标准芯片（副）
auto-battle-data-standard-chips = 标准芯片
auto-battle-data-mega-chips = MEGA 芯片
auto-battle-data-giga-chip = GIGA 芯片
auto-battle-data-combos = 连段
auto-battle-data-program-advance = 程序进阶

# Auto Battle Data editor
auto-battle-data-edit-used = 已使用
auto-battle-data-edit-secondary = 副
auto-battle-data-edit-count = { $count } 个芯片

# Common
save-empty = 此存档没有该视图的数据。
play-no-selection = 选择一个游戏和存档以查看。

# Replays
replays-filter-all-games = 所有游戏
replays-filter-opponent-placeholder = 搜索对手…
replays-show-incomplete = 显示未完成
replays-direct-marker = （直连）
replays-watch = 观看
replays-watch-missing-rom = 观看（未扫描此游戏的 ROM）
replays-export = 渲染
replays-export-progress = 正在渲染…
replays-export-cancel = 取消
replays-export-cancelling = 正在取消…
replays-export-success = 渲染完成。
replays-export-error = 渲染失败：{ $error }
replays-export-open = 打开渲染文件
replays-export-reset = 重置
replays-export-scale = 缩放
replays-export-scale-lossless = 无损
replays-export-disable-bgm = 静音
replays-export-twosided = 双方视角
replays-export-rounds = 回合：
replays-export-save-as = 另存为…
playback-close = 关闭
playback-play = 播放
playback-pause = 暂停
playback-options = 选项
playback-speed = 速度
playback-disconnect = 断开连接
playback-disconnect-prompt = 从此对战断开连接？
playback-disconnect-detail = 你将结束与对手的对战。
playback-cancel = 取消
playback-reconnecting = 连接已断开
playback-reconnecting-detail = 正在重新连接…
replays-select-prompt = 选择一个录像。
play-opponent = 对手
replays-match-type = 对战类型：
replays-duration = 时长：
replays-round-count = { $count } 个回合
replays-incomplete = 未完成
play-you = 自己

# Patches
patches-update = 更新
patches-updating = 正在更新…
patches-update-failed = 更新失败：{ $error }
patches-open-folder = 打开文件夹
patches-favorite = 收藏
patches-unfavorite = 取消收藏
patches-search-placeholder = 搜索补丁…
patches-select-prompt = 选择一个补丁。
patches-readme-placeholder = 此补丁没有 README。
patches-details-authors = 作者：
patches-details-license = 许可证：
patches-details-source = 来源：
patches-details-games = 支持的游戏：
patches-netplay-compatibility = 联机兼容性：

# Settings panel
settings-section-general = 常规
settings-section-graphics = 图形
settings-section-netplay = 联机
settings-section-audio = 音频
settings-volume = 音量
settings-disable-bgm-in-pvp = 联机对战时静音
settings-nickname = 昵称
settings-language = 语言
settings-data-path = 数据路径
settings-streamer-mode = 主播隐私模式
settings-section-experimental = 实验性功能
settings-enable-save-editor = 启用存档编辑器
settings-experimental-warning = 实验性功能可能损坏或破坏你的存档，可能随时被更改或移除，并且可能缺少使存档在联机对战中保持合法的检查。使用风险自负。
settings-section-about = 关于
settings-section-input = 输入
settings-input-press-key = 按下按键或按钮…
settings-input-add = 添加绑定
settings-input-reset = 恢复默认
settings-input-select-hint = 点击按键以编辑其绑定
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
input-key-speed-up = 快进
input-gamepad-south = A 键
input-gamepad-east = B 键
input-gamepad-west = X 键
input-gamepad-north = Y 键
input-gamepad-select = Select
input-gamepad-start = Start
input-gamepad-mode = 引导键
input-gamepad-left-thumb = 左摇杆
input-gamepad-right-thumb = 右摇杆
input-gamepad-left-shoulder = LB
input-gamepad-right-shoulder = RB
input-gamepad-dpad-up = 方向键 上
input-gamepad-dpad-down = 方向键 下
input-gamepad-dpad-left = 方向键 左
input-gamepad-dpad-right = 方向键 右
input-gamepad-misc1 = 其他 1
input-gamepad-misc2 = 其他 2
input-gamepad-misc3 = 其他 3
input-gamepad-misc4 = 其他 4
input-gamepad-misc5 = 其他 5
input-gamepad-misc6 = 其他 6
input-gamepad-right-paddle1 = 右背键 1
input-gamepad-left-paddle1 = 左背键 1
input-gamepad-right-paddle2 = 右背键 2
input-gamepad-left-paddle2 = 左背键 2
input-gamepad-touchpad = 触摸板
input-gamepad-axis-left-stick-x = 左摇杆 X
input-gamepad-axis-left-stick-y = 左摇杆 Y
input-gamepad-axis-right-stick-x = 右摇杆 X
input-gamepad-axis-right-stick-y = 右摇杆 Y
input-gamepad-axis-trigger-left = 左扳机
input-gamepad-axis-trigger-right = 右扳机
settings-theme = 主题
settings-theme-dark = 深色
settings-theme-light = 浅色
settings-accent = 强调色
settings-accent-tango-green = 探戈绿
settings-accent-megaman-blue = 洛克人蓝
settings-accent-protoman-red = 布鲁斯红
settings-accent-roll-pink = 罗尔粉
settings-accent-gutsman-yellow = 盖茨曼黄
settings-accent-bass-purple = 佛鲁特紫
settings-group-profile = 个人资料
settings-group-interface = 界面
settings-group-storage = 存储
settings-group-patches = 补丁
settings-group-updates = 更新
settings-group-window = 窗口
settings-group-emulator = 模拟器
settings-matchmaking-endpoint = 匹配服务器地址
settings-data-folder = 数据文件夹
settings-data-folder-change = 更改…
settings-patch-repo = 补丁仓库
settings-enable-patch-autoupdate = 在后台自动更新补丁
settings-enable-updater = 自动检查应用更新
settings-allow-prerelease-upgrades = 检查应用更新时包含预发布版本
settings-netplay-frame-delay = 帧延迟
settings-use-relay = 使用中继服务器
settings-use-relay-auto = 自动
settings-use-relay-always = 总是
settings-use-relay-never = 从不
settings-show-opponent-setup = 对战开始时显示对手的配置
settings-window-size = 窗口大小
settings-fullscreen = 全屏
settings-ui-scale = UI 缩放
settings-video-filter = 视频滤镜
settings-fractional-scaling = 分数缩放
settings-hide-emulator-border = 隐藏模拟器边框
updater-current-version = 当前版本：{ $version }
updater-latest-version = 最新版本：{ $version }
updater-loading = 检查中…
updater-up-to-date = v{ $version }（最新）
updater-downloading = 正在下载：{ $pct }%
updater-ready-to-update = 更新已下载，准备安装。
updater-update-now = 立即更新

# Welcome screen
welcome-title = 欢迎使用 Tango！
welcome-subtitle = 开始游玩前，你只需完成几个步骤。
welcome-continue = 继续
welcome-step-roms = 添加你的 ROM
welcome-step-roms-description = 将你的 Battle Network / Rockman EXE .gba 文件放入：
welcome-step-roms-detected = 检测到 { $count } 个 ROM。
welcome-step-nickname = 设置你的昵称
welcome-step-nickname-description = 你可以随时在设置中更改。
welcome-open-folder = 打开 ROM 文件夹
welcome-roms-needed = 继续前请至少添加一个 ROM。

# Common actions
rescan = 重新扫描

# Game names live in games.ftl — same Fluent attribute scheme the
# legacy app uses (game-<family> = base name; .variant-N for each
# regional/colour variant; .match-type-X-Y for per-mode labels).
