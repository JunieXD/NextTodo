# macOS

macOS App：Tauri 2 外壳加载打包在安装包里的网页端，加上菜单栏快速添加、全局快捷键、截止提醒和开机自启。总体要求见[总体设计](design.md#macos)，网页端见[网页端](web.md)，和 [Android](android.md) 共用“App 模式”的连接方式。

## 结构

| 位置 | 内容 |
|---|---|
| `apps/desktop` | Tauri 命令行（`pnpm tauri …`） |
| `apps/desktop/src-tauri` | Rust 外壳：窗口、菜单栏图标、全局快捷键、提醒。单独的 Cargo 工作区，不进根目录的工作区，免得 `cargo test` 每次都编译 Tauri |

网页端用 `window.__TAURI_INTERNALS__` 判断自己在 macOS App 里。界面和网页一样，只有下面几处不同。

## 连接服务器

和 Android 相同：登录页多“服务器”栏，HTTP 加 `Authorization: Bearer <令牌>`，WebSocket 在 `hello` 里带令牌，设备名是“NextTodo App · macOS”。

- 页面来自 `tauri://localhost`，和服务器不同源。HTTP 经 `@tauri-apps/plugin-http` 由 Rust 发出，不受跨域限制，服务端不需要 CORS。
- 令牌和服务器地址用 Capacitor Preferences 存（不在 Capacitor 里时它就是 `localStorage`），和 Android 共用一套代码；macOS 没有小组件要读它。
- HTTP 插件关掉了默认的 Cookie：服务端登录时照样设 Cookie，App 不存也不发，只用令牌。
- 页面只有监听事件、开机自启和 HTTP 请求这几项权限（`capabilities/default.json`）。HTTP 请求可以发往任意 `http(s)://` 地址和端口，因为服务器由用户填；范围里不写端口（`https://*`）时插件只放行默认端口。

## 窗口和菜单栏

- 一个主窗口，就是完整的网页端。标题栏透明，网页一直画到窗口顶端（`titleBarStyle: "Overlay"`，不显示标题），窗口的三个按钮浮在左栏顶部；左栏收起时，中栏顶部的按钮让到右边。左栏顶部、中栏的工具条和标题、手机布局的顶部栏可以拖动窗口，双击缩放（`data-tauri-drag-region`）。关窗口只是隐藏，App 留在菜单栏里继续同步和提醒；⌘Q 或菜单栏菜单里的“退出”才退出。
- 窗口隐藏后网页照常运行（`backgroundThrottling: "disabled"`）。WebKit 默认在页面隐藏几分钟后挂起它，同步和提醒的交接就停了；这个设置要 macOS 14，所以最低支持 macOS 14。
- 菜单栏图标（单色模板图，随深浅色变化）：左键点一下打开主窗口并聚焦快速添加；右键菜单有“快速添加”“打开 NextTodo”“退出”。
- 全局快捷键 ⌃⌥Space：在任何应用里都能呼出主窗口并聚焦快速添加。
- 快速添加：Rust 向页面发 `quick-add` 事件，页面打开 `/?add`，和 Android 的 `nexttodo://add` 一样；开着的设置面板和搜索会让开。
- 要开新窗口的链接（描述里的链接都是）交给系统默认的浏览器或邮件 App，只放行 `http`、`https`、`mailto`。为了接住这类请求，主窗口在 `tauri.conf.json` 里设为 `create: false`，由 Rust 按这份配置创建。

## 截止提醒

只在 App 运行时提醒（关掉窗口也算运行）：

- 会话开始时和本地副本变化后（第一次变化后 2 秒内交接一次，合并这期间的变化），按和 Android 相同的规则算出将来的所有提醒，整份交给 Rust（`set_reminders` 命令）。Android 因为系统限制闹钟数量只排最近 50 个，这里没有这个限制。
- Rust 用一个线程等到最近的提醒时间，弹出系统通知；列表换了就重新等。等待的计时在 Mac 睡眠时停走，所以最长 30 秒看一次钟。每次交接还带上上次交接以来到点、事项仍然需要的提醒。Rust 记着线程上次看钟的时间：在它之前到点的已经处理过（该弹的弹了），丢掉；之后的按新列表保留（时钟往回调了的，以当前时间为准）。所以 Mac 刚睡醒时到点的照样弹一次，内容是最新的；期间完成、删除或改期了的不弹。
- 退出登录时清空提醒（`clear_reminders`），已经到点、还没弹出的也清掉。
- 点提醒或提醒上的“打开”按钮，打开主窗口和这个事项，和 Android 一样（开着的设置面板会关掉）。macOS 的通知只有带按钮时才会等用户点击并报告，所以提醒都带这个按钮；通知插件为此每条通知开一个线程，等到用户处理它，提醒一天没几条，可以接受。

## 开机自启

设置页“开机自启”开关，用 `tauri-plugin-autostart` 写 LaunchAgent，读写失败时提示。开机启动时带 `--hidden`，只出现在菜单栏，不弹窗口，也不占 Dock；打开窗口以后才出现在 Dock 里。

## 构建

- `pnpm tauri build`（在 `apps/desktop` 下）：构建网页端，再打 `.app` 和 `.dmg`。
- 不做苹果签名，用 ad-hoc 签名（`signingIdentity: "-"`）。本机构建的不带隔离属性，直接能打开；从别处下载的第一次打开会被拦下，要到“系统设置 → 隐私与安全性”里点“仍要打开”。
- 版本号用网页端 `package.json` 的版本。

## 测试

提醒时间的计算和 Android 共用，已有 Vitest。外壳在本机实际操作验证：登录、同步、关窗口后继续同步、菜单栏和快捷键快速添加、提醒到点弹出、开机自启开关、退出登录后提醒清空。
