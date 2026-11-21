# iOS

iOS App（iPhone 和 iPad）：和 [Android](android.md) 一样，Capacitor 外壳加载打包在安装包里的网页端，加上截止提醒和[本机模式](local.md)，不做小组件。不上架 App Store：发布不签名的 IPA，用户用 SideStore 和自己的 Apple ID 安装。总体要求见[总体设计](design.md#ios)。

## 结构

| 位置 | 内容 |
|---|---|
| `apps/web/capacitor.config.ts` | Capacitor 配置，和 Android 共用（`ios.path` 指到 `apps/ios`） |
| `apps/ios` | Xcode 工程：外壳、截止提醒、给网页调用的插件（Swift）。依赖用 Swift Package Manager，不用 CocoaPods |
| `crates/ios` | 本机模式的服务端，编译成静态库链接进 App，见[本机模式](#本机模式) |

- 最低支持 iOS 16.4：页面跑在系统的 WKWebView 里，内核随 iOS 更新，16.4 正是网页端的[最低要求](web.md#浏览器)（Safari 16.4）。能装 iOS 16 的是 iPhone 8 及以后的机型。
- 网页端用 `Capacitor.getPlatform()` 区分 Android 和 iOS（`api/http.ts` 的 `app`，两个都算的是 `phoneApp`）。两个 App 共用插件 `NextTodo` 的接口（`apps/web/src/mobile.ts`），iOS 只实现用得上的方法：截止提醒、原生发出的改动、本机模式、备份恢复、附件、版本和打开系统设置。小组件、写进系统日历、弹窗提醒、精确闹钟、网页热更新、安装 APK 和弹出键盘是 Android 专有的，网页端在 iOS 上不调用。

## 连接服务器

和 [Android](android.md#连接服务器) 相同，以下几处不同：

- 页面来自 `capacitor://localhost`：WKWebView 不能接管 `https` 这个协议，Capacitor 在 iOS 上只能用自己的协议名。请求照样经原生 HTTP（`CapacitorHttp`）发出，不受跨域限制；附件的文件用 WKWebView 自己的 `fetch`，见[附件](#附件)。
- 令牌和服务器地址存在 Capacitor Preferences（iOS 的 `UserDefaults`，App 私有）。iOS 不能让 App 单独不参加 iCloud 备份，它们随整机备份（加密）一起备份；从备份恢复到新手机后照样登录着。
- 设备名是“NextTodo App · iOS”。
- 没有返回键。对话框、弹窗和详情照常点“×”或“返回”关掉。
- 键盘弹出时 WebView 随之变矮（`@capacitor/keyboard` 的 `resize: "native"`），和 Android 一样，顶栏不动，底部的快速添加栏留在键盘上面；不然 iOS 会把整个页面往上推，顶栏推出屏幕。插件还隐藏了键盘上方的表单辅助栏（↑ ↓ ✓），点输入框以外的地方收起键盘。这个插件只用在 iOS（`android.includePlugins` 里没有它），Android 的 WebView 自己会变矮。
- 状态栏的文字颜色随 App 里选的主题变（Capacitor 的 `SystemBars`，`theme.ts`），否则 App 选了深色、系统是浅色时，状态栏会是深色底上的深色字。
- 启动画面是 Android 启动画面的绿色，中间是 88pt 的标志；图标和启动画面的标志由 `apps/web/scripts/icons.js` 生成，见[界面](ui.md#产品图标)。

## 截止提醒

网页算出提醒，交给原生排定，和 [Android](android.md#截止提醒) 相同：同样的时机、同样的列表（`setReminders`）。不同在于到点由系统弹出，App 不用运行，原生不用闹钟：

- 用系统的本地通知（`UNUserNotificationCenter`），每个提醒排一个通知，以事项 ID 加提醒时刻为标识，同一事项的归成一组（`threadIdentifier`）。iOS 不能让后到点的换掉已经弹出的，所以一个事项有几个提醒时，通知栏里可能有它的几条；在通知上完成时，这一次（截止时刻相同）的全部收回，排着的也取消，重复事项下一次的留着。这一次已经完成了的（从它的另一条通知上，或在别处完成、改期后交来了列表），再从它的通知上点“完成”不再完成一次，免得重复的事项多顺延一次。系统最多给一个 App 排 64 个，所以只排还没到的、最近的 64 个；每次交来列表时整份重排，排不下的等下次交接时补上。没打开 App 期间，最多响这 64 次。
- 能不能直接“完成”，在排定时就定下：到提醒时刻还在等前置的不能，同 [Android](android.md#截止提醒)；没到[开始时间](design.md#开始时间)的，按提醒时刻和开始时间比较（到点时 App 不运行，判断不了）。之后在别处加了前置的，要等 App 下次交接才改过来。
- 列表里没有了的、提醒时刻变了的，通知栏里已经弹出的也收回。提醒时刻没变、标题或内容变了的（改了标题、文件夹，打开了[隐藏通知内容](design.md#提醒)），或截止时间改了的，也收回：iOS 改不了已经弹出的通知，留着的是过时的内容，隐藏以后还会被人看到。只是能不能直接完成变了的，保持原样。
- 第一次交来不为空的列表时请求通知权限。提醒排的是具体时刻，改了手机时区，要等 App 下次回到前台才按新时区重排。
- App 在前台时到点的照样弹出横幅。
- 退出登录时清空排着的、推迟着的和通知栏里已经弹出的。

### 通知

- 标题、内容同 Android。点通知进入 App 并打开这个事项（经 Capacitor 的 `appUrlOpen`，地址同 Android 的 `nexttodo://task/<id>`）。
- 长按或下拉通知出现操作：“完成”（不打开 App，改动见[原生发出的改动](#原生发出的改动)），“15 分钟后提醒”“1 小时后提醒”“3 小时后提醒”。等待中的事项没有“完成”。
- 没有提醒卡片和自定义推迟：iOS 的通知操作只能是固定的按钮，App 也不能在别的应用上层弹出界面。三档取自 Android 稍后提醒的预设。
- 推迟的规则同 Android 的[稍后提醒](android.md#稍后提醒)：只推迟这台手机上的这一次，记在原生的 `UserDefaults` 里；提醒时刻没变的，推迟照旧，变了作废。一个事项同时只有一条推迟，后推迟的换掉先推迟的。推迟到的时刻之前同一事项还有下一个提醒的，推迟作废，下一个照常弹出：App 不运行时也一样，排定时就不排这条推迟。推迟的通知按列表里现在的内容重新生成，推迟期间打开了隐藏通知内容的，再弹出时也是隐藏的。
- 通知的代理（`UNUserNotificationCenterDelegate`）在 App 启动时就设好（`ios.handleApplicationNotifications: false`，不用 Capacitor 的），所以 App 没在运行、由通知操作在后台启动时也收得到。

### 原生发出的改动

通知上的“完成”不经过网页，由原生调用 `POST /mutations`，规则同 [Android](android.md#原生发出的改动)：原生生成改动 ID，按用户记下；连不上服务器、5xx、超时或限流（408、429）时先存下（`UserDefaults`）；App 打开会话和回到前台时把它们接过去；补发时才被拒绝的，发一条通知写明是哪一项、为什么。

- 存下的格式：`UserDefaults` 里一份 JSON `{"format": 1, "changes": [{id, task, today, what, user}]}`，`task` 是完成的事项，`today` 是点完成那天，`what` 是通知里写的“完成“交房租””，`user` 是当时登录的人。读不了的（格式不对，或 `format` 不是 1）当作没有，不会让 App 崩溃，也不会照旧的意思发出新版本存的改动；以后改格式时升 `format`，把旧的读进来。
- 存下的在下次点“完成”和 App 回到前台时补发。iOS 没有 WorkManager 那样联网后叫醒 App 的办法，不在后台自己补发；App 回到前台时页面也会接过去发出，两边由服务端按 ID 去重。
- 系统在后台处理通知操作时给 App 约 30 秒。本机模式下，这时服务端还没启动的，先启动它。

## 本机模式

服务端和 Android 一样在 App 的进程里运行，只监听 `127.0.0.1`，页面经 HTTP 和 WebSocket 连它，见[本机模式](local.md)。iOS 上不同的地方：

- 编译成静态库（`crates/ios`，`staticlib`），链接进 App：真机 `aarch64-apple-ios`，Apple 芯片 Mac 上的模拟器 `aarch64-apple-ios-sim`，只出 arm64。Xcode 构建前由脚本调用 `cargo` 编译（`apps/ios/rust.sh`），打包还是一条命令；正式版用 profile `ios`，和 `android` 一样优化体积。
- C 接口：`nexttodo_start`、`nexttodo_stop`、`nexttodo_session`、`nexttodo_pull`、`nexttodo_backup`、`nexttodo_prepare`、`nexttodo_commit`、`nexttodo_file_name`，对应 Android 的 `LocalServer`，Swift 一侧是 `LocalServer.swift`。会失败的返回一段 JSON（`{"value": …}`，或带着给用户看的原因的 `{"error": …}`），由 Swift 交还 `nexttodo_free` 释放；`nexttodo_stop` 和 `nexttodo_pull` 不返回。和 `crates/android` 一样，这个 crate 默认禁止 `unsafe`，只在每个入口和读 C 字符串的地方单独放行。
- 服务端的日志写进系统日志（`os_log`，子系统 `cn.juniexd.nexttodo`），Xcode 和“控制台”里看得到。
- 数据目录是 `Library/Application Support/local/`，结构同[启动](local.md#启动)。它随整机备份进 iCloud，恢复到新手机后数据还在。
- **什么时候启动**：页面向原生要地址时（插件的 `openLocal`），或在后台处理通知上的“完成”时，谁先要谁启动，另一个接着用；已经启动的，先按下面[回到前台](#本机模式)的办法确认端口还是自己的，再给出地址。签发会话、按[数据代号](local.md#数据代号)清掉旧数据的改动和提醒，同 Android 的 `Local.open`。
- **重启**：iOS 的 App 不能自己重新打开。进入、离开本机模式，恢复备份，借网盘同步换成网盘里的数据时，Android 和桌面重启整个 App；iOS 改为在进程里停下服务端（`nexttodo_stop`），再重新加载页面：页面加载时照常向原生要地址，服务端随之重新启动，启动时完成恢复或换上网盘的数据。
  - 停下就是结束服务端的运行时：正在处理的请求和后台任务（借网盘同步、AI 整理）当场中断，和 Android、桌面重启时结束进程一样，它们本来就经得起中断。已经交给阻塞线程的工作（数据库事务、从网盘下载附件等）停不下来，在后台做完后才放开数据库和[数据目录锁](local.md#启动)；再启动时照常等锁，最多 10 秒，等不到就显示启动失败和“重试”，同新进程等旧进程。
- **回到前台**：App 在后台会被系统挂起，服务端随之停住，回来接着运行，和 Android 被冻结一样。挂起一会儿，系统就会收回 App 正在监听的端口（Apple 的 TN2277；真机上锁屏后一两分钟到几分钟就收回了），回来时就连不上了。所以回到前台时先确认自己的端口还是自己的：别的程序可能在这期间占了它，也会回应，而页面和原生的请求带着令牌。服务端就在 App 的进程里，由它自己连一下自己的地址，带上一个随机值，再看自己有没有收到；没收到（连不上，或者回应的是别的程序），就停下服务端，在同一个端口上重新启动：页面的地址和令牌不变，同步连接断开后照常重连；同一个端口用不了的，重新加载页面，拿新的地址。
  - 页面也不抢在前面：从页面隐藏起（加载时就是隐藏的也算），它发往本机服务端的请求和同步连接都先等着；回到前台时再向原生要一次地址（`openLocal`，同样查一遍端口），还是原来的地址和令牌才发出；换了的，或服务端起不来的，页面重新加载，在这之前什么都不发。后来又隐藏过的，等最后一次的确认。确认排在原生正在做的事后面（比如要从网盘下载附件的备份），可能要等一阵：等着的请求到了自己的时限照常失败，同步连接接着等。
  - 在后台处理通知上的“完成”时也一样：App 被叫醒时端口可能早已收回，先这样查一遍再发改动，不然改动只能存着，等下次打开 App 才发出。同一个端口用不了的，改动先存下，不发往旧地址。
  - 回到前台时也马上看一次网盘（[借网盘同步](local.md#运行)），同 Android 的 `onResume`。
- **明文 HTTP**：App Transport Security 允许明文，见[直接用 HTTP](design.md#直接用-http)。页面（`capacitor://localhost`）连 `http://127.0.0.1` 和 `ws://127.0.0.1` 不算混合内容。本机的服务端允许来源 `capacitor://localhost` 跨域，同 Android 的 `https://localhost`。
- 和 Android 一样：网页只用安装包自带的；设置里不显示令牌一组，agent 接不进来；后台挂起期间 AI 整理暂停，进程被结束的下次启动时记为失败。

## 附件

- 页面上传、下载附件的文件用 WKWebView 自己的 `fetch`（`window.CapacitorWebFetch`），连服务器和本机模式都一样，理由同 [Android](android.md#附件)。服务端的附件接口允许来源 `capacitor://localhost` 跨域，见 [API](api.md#附件)；加上 iOS App 以前的服务端不允许，要先升级服务器，否则上传、下载失败。
- 打开和保存文件：页面同样分块交给插件的 `writeAttachment`，写进临时目录的 `attachments/<附件 ID>/<名称>`，规则同 Android。然后：
  - `openAttachment`：用系统的“快速查看”（`QLPreviewController`）打开，在那里可以分享给别的 App；看不了的文件也能分享出去。
  - `saveAttachment`：用系统的文件选择器（`UIDocumentPickerViewController`，导出副本）让用户选位置，存进“文件”App 里。
  - 这些文件在 App 下次启动时删掉。
- 拍照用 `<input type="file" accept="image/*" capture="environment">`，WKWebView 自己打开相机；`Info.plist` 写明用途（`NSCameraUsageDescription`）。

## 备份和恢复

规则见[备份和恢复](local.md#备份和恢复)，iOS 上只是选文件的界面不同：

- 备份：服务端先写进临时目录，再用系统的文件选择器导出副本，用户选位置；取消了就删掉临时文件。
- 恢复：用系统的文件选择器选一个备份，复制进临时目录后交给服务端准备。确认后提交，再按上面的[重启](#本机模式)重新加载页面。

## 设置

- “提醒和后台”在 iOS 上叫“提醒”，只有一行“通知”，同 Android：允许了写“已允许”；没允许时写“未允许时不会弹出截止提醒”，附“允许”，还没问过的弹出系统的请求，拒绝过的打开本 App 的通知设置。iOS 不需要自启动、省电和精确闹钟的设置。
- “关于”见[更新](#更新)。

## 更新

- 网页打包在 App 里，随整包更新，不热更新，和桌面一样。
- 版本：`CFBundleShortVersionString` 是 `0.1.<提交数>`，`CFBundleVersion` 是提交数，构建时写入；和同一次构建的 Android、桌面 App 一样。Xcode 直接构建的开发版是 0.1.0（构建号 0），和网页端的开发版一致。
- App 不能自己安装新版本，只提醒：打开 App、回到前台时和运行期间（每 6 小时最多一次），从 GitHub 最新 Release 取 `sidestore.json`（`https://github.com/JunieXD/NextTodo/releases/latest/download/sidestore.json`，同一份文件也是 SideStore 的软件源）。里面的构建号（`buildVersion`）比已装的（`CFBundleVersion`）大时，设置“关于”里显示“有新版本”，下面写“0.1.N，在 SideStore 里更新”，打开 App 时用提示条提醒一次。检查失败时的处理和“关于”里的显示同[总体设计](design.md#app-更新)，连不上时写“无法连接 GitHub”。
- 连服务器时也从 GitHub 取，不从服务器取：服务器上没有 iOS 的安装包，SideStore 也只认这一个软件源。所以 iOS App 可能比服务器新，服务器跟着 Release 升级即可（管理员在“关于”里一键升级）。

## 构建和发布

- 需要 Xcode（不必设为当前的开发者目录，脚本默认用 `/Applications/Xcode.app`，可以用 `DEVELOPER_DIR` 另指）、`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`。
- `pnpm ios:build`（在 `apps/web` 下）：构建网页端、`cap sync ios`，再由 `apps/ios/build.sh` 用 `xcodebuild archive` 构建不签名的正式版（`CODE_SIGNING_ALLOWED=NO`），打成 `apps/ios/build/NextTodo.ipa`。
- `xcodebuild` 都带 `-packageAuthorizationProvider netrc`：依赖都是公开的，不加的话 SwiftPM 去钥匙串里找 GitHub 的账号，可能停在一个看不见的授权弹窗上。
- `deploy/build.sh` 把 IPA（`NextTodo-ios.ipa`）和 `sidestore.json` 放进 Release 的文件，见[部署](deploy.md#构建发布)。`sidestore.json` 是 SideStore（AltStore 格式）的软件源，只列这一个版本，下载地址写成这个 Release 的完整地址；权限（`appPermissions`）从构建出的 App 的 `Info.plist` 读：和 IPA 对不上的，SideStore 拒绝安装。
- **安装**：用户在 iPhone 上装好 SideStore，加入软件源 `sidestore.json` 的地址，在那里安装 NextTodo，以后也在那里更新。SideStore 用用户自己的 Apple ID 给 App 签名，免费账号签的 7 天后到期，SideStore 在到期前续签（打开 SideStore 或它在后台续签），过期了 App 打不开，续签后数据还在。步骤见官网的[下载和安装](https://juniexd.github.io/NextTodo/guide/install#ios)。
- 不做 App Store 和 TestFlight，不付费开发者账号：上架要审核和年费，TestFlight 每 90 天要重新发。

## 不做

- 小组件。
- 弹窗提醒、精确闹钟、自启动和省电设置：iOS 没有，也不需要。
- 写进系统日历：第一版不做。
- 网页热更新、App 自己安装新版本。
- agent 接入：手机上没有命令行，同 Android。
- 推送通知：提醒都是本地排定的。

## 测试

- 在 Apple 芯片 Mac 的模拟器上跑。模拟器的运行时用 `xcodebuild -downloadPlatform iOS` 下载。
- 构建并装到模拟器：`xcodebuild -project apps/ios/App/App.xcodeproj -scheme App -destination id=<模拟器 ID> -derivedDataPath apps/ios/build/sim -packageAuthorizationProvider netrc build`，再 `xcrun simctl install`、`xcrun simctl launch`。要指定模拟器：不指定时也构建 x86_64，而服务端的静态库只有 arm64，链接失败。页面可以用 Safari 的“开发”菜单调试（调试版 `isInspectable`）。
- 截止提醒：设一个一两分钟后的提醒，在模拟器里等它弹出，长按试通知上的操作。

已验证的（2026-11-18，iOS 27.2 模拟器）：服务端编译到两个目标并在 App 里启动；页面连本机的 `fetch`（带 `Authorization`，有预检）和 WebSocket；停下后在同一进程里、同一端口上再启动（`crates/ios` 的测试）；进入、离开本机模式和恢复备份时的重新加载；截止提醒到点弹出，通知上的“完成”（本机模式和连服务器）、推迟和点开事项；退出登录清空提醒；附件跨域上传下载、快速查看、存进“文件”；备份和恢复；键盘。

真机上已验证的（2026-11-18，iPhone SE 第 2 代，iOS 26.3.1）：不签名的 IPA 用 SideStore 和免费 Apple ID 装上、续签，续签后数据还在；本机模式的服务端启动，页面连上它；锁屏后一两分钟到几分钟端口就被收回，回到前台时在同一端口重新启动，地址和令牌不变；端口收回后在锁屏上点通知上的“完成”，改动在后台直接发出；通知上的“完成”和推迟；拒绝过通知后，“允许”直接打开本 App 的通知设置；小屏（4.7 英寸，有 Home 键）的布局。

用数据线连着 Mac 时：经 usbmuxd 连手机上 `127.0.0.1` 的端口（同 `iproxy`），直接访问本机的服务端；崩溃日志用 `xcrun devicectl device info files --domain-type systemCrashLogs` 列出，`device copy from` 取回。

还要在真机上验证：软件源能不能加入（要等 Release 里有 `sidestore.json`）。
