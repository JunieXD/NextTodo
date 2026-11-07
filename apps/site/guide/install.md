# 下载和安装

NextTodo 有 Android、macOS 和 Windows App。连接自己的服务器时，也可以在浏览器里使用网页版。下面的下载链接总是最新版。

## Android

要求 Android 8.0 及以上。

1. 用手机浏览器打开 [NextTodo-android.apk](https://github.com/JunieXD/NextTodo/releases/latest/download/NextTodo-android.apk)，下载安装包。
2. 下载完成后点开它。第一次安装从浏览器下载的应用时，系统会提示不允许安装未知来源的应用：点击“设置”，打开“允许来自此来源的应用”，返回后继续安装。各品牌手机的叫法略有不同。
3. 打开 NextTodo，[选择用法](./#modes)。

要按时收到截止提醒，打开 App 的“设置 → 提醒和后台”，按提示允许“通知”和“准时提醒”。小米、红米等使用澎湃 OS 的手机，还要点“自启动和省电”一行的“打开设置”，允许自启动，并把省电策略设为“无限制”。

桌面小组件：长按手机桌面的空白处，选择“小组件”，找到 NextTodo，拖到桌面上。小组件显示“接下来”，可以直接勾选完成或添加事项。

## macOS

要求 Apple 芯片（M1 及以后），macOS 14 及以上。

1. 下载 [NextTodo-macos-arm64.dmg](https://github.com/JunieXD/NextTodo/releases/latest/download/NextTodo-macos-arm64.dmg)，双击打开。
2. 把窗口里的 NextTodo 拖进“应用程序”文件夹。不要直接在这个窗口里打开它，否则以后无法安装命令行。
3. 打开“应用程序”文件夹，双击 NextTodo。第一次打开时，系统会提示无法验证开发者：
   1. 点击“完成”关掉提示。
   2. 打开“系统设置 → 隐私与安全性”，在页面下方找到关于 NextTodo 的提示，点击“仍要打开”，按提示输入登录密码。
   3. 之后可以正常打开，不会再提示。

   出现提示是因为 NextTodo 没有向苹果付费做签名公证，不代表它有问题。

4. [选择用法](./#modes)。

关掉窗口后，NextTodo 仍在菜单栏里，继续同步和提醒。点菜单栏的图标打开快速添加，右键图标选“退出”才会退出。在任何应用里按 ⌃⌥空格（Control + Option + 空格）也能快速添加。要开机时自动运行，在设置里打开“开机自启”。

## Windows

要求 Windows 10 或 11，x64。

1. 下载 [NextTodo-windows-x64-setup.exe](https://github.com/JunieXD/NextTodo/releases/latest/download/NextTodo-windows-x64-setup.exe)，双击运行。
2. 出现“Windows 已保护你的电脑”时，点击“更多信息”，再点击“仍要运行”。原因和 macOS 一样，安装程序没有付费签名。
3. 按提示完成安装。安装程序只装给当前用户，不需要管理员权限；系统缺少 WebView2 时会自动下载。
4. [选择用法](./#modes)。

关掉窗口后，NextTodo 仍在任务栏右下角的托盘里，继续同步和提醒。右键托盘图标选“退出”才会退出。在任何应用里按 Ctrl + Alt + 空格也能快速添加。要开机时自动运行，在设置里打开“开机自启”。

## 网页版

只在连接服务器时可用：用浏览器打开服务器的域名，登录即可。

网页版可以安装成独立的窗口：Chrome 和 Edge 点地址栏右侧的“安装”图标；iPhone 和 iPad 的 Safari 点“分享 → 添加到主屏幕”。网页版不弹出截止提醒，需要提醒的请用 App。

## 更新

不用手动更新：

- **连接服务器时**：App 从服务器获取更新，服务器升级后随之更新。Android 下次打开时生效，需要换安装包时会提示；电脑上在后台下载，提示“重启”后生效。
- **在本机使用时**：App 从 GitHub 获取更新，有新版本时同样会提示。

版本号在“设置 → 关于”里查看。
