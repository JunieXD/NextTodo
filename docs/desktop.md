# 桌面 App

桌面 App 有 macOS 版和 Windows 版：Tauri 2 外壳加载打包在安装包里的网页端，加上快速添加、全局快捷键、截止提醒和开机自启。总体要求见[总体设计](design.md#桌面macos-和-windows)，网页端见[网页端](web.md)，和 [Android](android.md) 共用“App 模式”的连接方式。

## 结构

| 位置 | 内容 |
|---|---|
| `apps/desktop` | Tauri 命令行（`pnpm tauri …`） |
| `apps/desktop/src-tauri` | Rust 外壳：窗口、菜单栏或托盘图标、全局快捷键、提醒、更新。单独的 Cargo 工作区，不进根目录的工作区，免得 `cargo test` 每次都编译 Tauri |

网页端用 `window.__TAURI_INTERNALS__` 判断自己在桌面 App 里，用 User-Agent 区分 macOS 和 Windows。界面和网页一样，只有下面几处不同。

## 连接服务器

也可以不连服务器，在本机使用，见[本机模式](local.md)。

和 Android 相同：登录页多“服务器地址”栏，HTTP 加 `Authorization: Bearer <令牌>`，WebSocket 在 `hello` 里带令牌，设备名是“NextTodo App · macOS”或“NextTodo App · Windows”。

- 页面来自 Tauri 的本地地址，和服务器不同源。HTTP 经 `@tauri-apps/plugin-http` 由 Rust 发出，不受跨域限制，服务端不需要 CORS。
- 令牌和服务器地址用 Capacitor Preferences 存（不在 Capacitor 里时它就是 `localStorage`），和 Android 共用一套代码；桌面没有小组件要读它。
- HTTP 插件关掉了默认的 Cookie：服务端登录时照样设 Cookie，App 不存也不发，只用令牌。
- 页面只有监听事件、开机自启和 HTTP 请求这几项权限（`capabilities/default.json`）。HTTP 请求可以发往任意 `http(s)://` 地址和端口，因为服务器由用户填；范围里不写端口（`https://*`）时插件只放行默认端口。

## 窗口和图标

- 一个主窗口，就是完整的网页端。关窗口只是隐藏，App 留在菜单栏（macOS）或系统托盘（Windows）里继续同步和提醒；菜单里的“退出”才退出，macOS 上 ⌘Q 也可以。
- macOS 的标题栏透明，网页一直画到窗口顶端（`titleBarStyle: "Overlay"`，不显示标题），窗口的三个按钮浮在左栏顶部；左栏收起时，中栏顶部的按钮让到右边。左栏顶部、中栏的工具条和标题、手机布局的顶部栏可以拖动窗口，双击缩放（`data-tauri-drag-region`）。Windows 用系统标题栏。
- 窗口隐藏后网页照常运行（`backgroundThrottling: "disabled"`）。macOS 的 WebKit 默认在页面隐藏几分钟后挂起它，同步和提醒的交接就停了；这个设置要 macOS 14，所以最低支持 macOS 14。
- 菜单栏或托盘图标：左键点一下打开主窗口并聚焦快速添加；右键菜单有“快速添加”“打开 NextTodo”“退出”。macOS 用单色模板图，随菜单栏深浅变化；Windows 用 App 图标。
- 全局快捷键：macOS 是 ⌃⌥Space，Windows 是 Ctrl+Alt+Space，在任何应用里都能呼出主窗口并聚焦快速添加。被别的应用占用时快捷键不起作用，图标照常可用。
- 快速添加：Rust 向页面发 `quick-add` 事件，页面打开 `/?add`，和 Android 的 `nexttodo://add` 一样；开着的设置面板和搜索会让开。
- 要开新窗口的链接（描述里的链接都是）交给系统默认的浏览器或邮件 App，只放行 `http`、`https`、`mailto`。为了接住这类请求，主窗口在 `tauri.conf.json` 里设为 `create: false`，由 Rust 按这份配置创建。
- 只运行一个实例：Windows 上再次打开 App 时，显示已在运行的那个的窗口（`tauri-plugin-single-instance`）；macOS 本来就这样。

## 截止提醒

只在 App 运行时提醒（关掉窗口也算运行）：

- 会话开始时和本地副本变化后（第一次变化后 2 秒内交接一次，合并这期间的变化），按和 Android 相同的规则算出将来的所有提醒，整份交给 Rust（`set_reminders` 命令）。Android 因为系统限制闹钟数量只排最近 50 个，这里没有这个限制。
- Rust 用一个线程等到最近的提醒时间，弹出系统通知；列表换了就重新等。等待的计时在电脑睡眠时停走，所以最长 30 秒看一次钟。每次交接还带上上次交接以来到点、事项仍然需要的提醒。Rust 记着线程上次看钟的时间：在它之前到点的已经处理过（该弹的弹了），丢掉；之后的按新列表保留（时钟往回调了的，以当前时间为准）。所以电脑刚睡醒时到点的照样弹一次，内容是最新的；期间完成、删除或改期了的不弹。
- 退出登录时清空提醒（`clear_reminders`），已经到点、还没弹出的也清掉。
- 点提醒或提醒上的“打开”按钮，打开主窗口和这个事项，和 Android 一样（开着的设置面板会关掉）。macOS 的通知只有带按钮时才会等用户点击并报告，所以提醒都带这个按钮；通知插件为此每条通知开一个线程，等到用户处理它，提醒一天没几条，可以接受。

## 更新

总体见[总体设计](design.md#app-更新)。用 `tauri-plugin-updater`：

- 版本 `0.1.<提交数>`，由 `deploy/build.sh` 打包时写入。`tauri build` 同时生成更新包和它的签名（`createUpdaterArtifacts`）：macOS 是 `NextTodo.app.tar.gz`，Windows 直接用安装程序。构建脚本据此写出 `/app/desktop.json`，其中的下载地址是相对这个文件的路径，所以同一个镜像部署在哪个域名下都能用。
- 启动时和之后每 6 小时，Rust 取 `<登录的服务器>/app/desktop.json`，把下载地址换成完整地址，交给更新插件判断版本、下载并校验签名。签名里记着版本号，必须和文件里写的一致。
- 下载好以后，macOS 当场替换 App，下次启动生效；Windows 的安装程序一运行就会关掉 App，所以先留在内存里，等点了“重启”再安装（被动模式，只显示进度条），装完自动打开。提示条显示“新版本已准备好，重启后生效”，附“重启”；macOS 不点的话，下次退出再打开时生效，Windows 不点的话，退出后这次的下载作废，下次启动重新检查。
- 更新密钥：私钥文件路径和密码从环境变量 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 读取，不进仓库；公钥写在 `tauri.conf.json`。私钥丢了，已装的 App 就收不到更新，只能手动装一次换了公钥的新版本。
- App 装在哪就在哪替换。

设置页“关于”显示网页版本和 App 版本。

## 开机自启

设置页“开机自启”开关，用 `tauri-plugin-autostart`（macOS 写 LaunchAgent，Windows 写注册表的启动项），读写失败时提示。开机启动时带 `--hidden`，只出现在菜单栏或托盘，不弹窗口；macOS 上也不占 Dock，打开窗口以后才出现在 Dock 里。

## 构建

都在 Mac 上构建，在 `apps/desktop` 下运行。更新包要用更新密钥签名，打包前设好[更新](#更新)里的两个环境变量，没有它们打包会失败。

| 平台 | 命令 | 产物 |
|---|---|---|
| macOS（Apple 芯片） | `pnpm tauri build --bundles app,dmg` | `.app`、`.dmg` 和更新包 |
| Windows（x64） | `pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis` | NSIS 安装程序和它的签名 |

- Windows 版用 `cargo-xwin` 交叉编译，要 `rustup target add x86_64-pc-windows-msvc`、`cargo install cargo-xwin`、Homebrew 的 `nsis` 和 `llvm`（`llvm` 的 `bin` 要在 `PATH` 里，里面有 `clang-cl` 和 `llvm-rc`）。第一次构建会下载 Windows SDK，可以用 `XWIN_CACHE_DIR` 指定放在哪里。
- 不做苹果签名和公证，用 ad-hoc 签名（`signingIdentity: "-"`）。本机构建的不带隔离属性，直接能打开；从网上下载的第一次打开会被拦下，要到“系统设置 → 隐私与安全性”里点“仍要打开”。
- Windows 安装程序没有代码签名，第一次运行时 SmartScreen 会提示，点“更多信息 → 仍要运行”。默认装到当前用户目录，不需要管理员权限；系统缺 WebView2 时安装程序会下载。
- 版本号：`deploy/build.sh` 打包时是 `0.1.<提交数>`（见[更新](#更新)）；直接用上面的命令打出的是网页端 `package.json` 的版本（0.1.0），装上后会被服务器上的版本更新掉。

## 测试

提醒时间的计算和 Android 共用，已有 Vitest。外壳在本机实际操作验证：登录、同步、关窗口后继续同步、菜单栏和快捷键快速添加、提醒到点弹出、开机自启开关、退出登录后提醒清空、更新。

Windows 版在 Windows 11 虚拟机（ARM 版，x64 模拟运行）里验证过：登录、同步、关窗口后隐藏、托盘图标和菜单、快捷键快速添加、提醒弹出和点击打开、开机自启开关、只运行一个实例、更新。调试时：

- 操作页面：启动前设环境变量 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`，在虚拟机里经 CDP（`http://127.0.0.1:9222/json`）连接。
- 查看日志和 panic：正式版没有控制台窗口，从命令行启动 `nexttodo-desktop.exe`，把标准错误重定向到文件。
- 测试更新：正式版的更新地址必须是 `https`。用本机的 `http` 服务器测试时，先装一个设了 `plugins.updater.dangerousInsecureTransportProtocol` 的测试包，再从它更新。
