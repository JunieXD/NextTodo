# 快速上手

NextTodo 是一个待办应用。和普通待办不同的是，事项之间可以设置先后：“建好官网”完成后才能“写官网文档”。NextTodo 据此只把现在能做的事列在“接下来”里，完成一项，它解锁的事项随即出现。

## 1. 安装 App

| 平台 | 下载 | 要求 |
|---|---|---|
| Android | [NextTodo-android.apk](https://github.com/JunieXD/NextTodo/releases/latest/download/NextTodo-android.apk) | Android 8.0 及以上 |
| iOS | 用 SideStore 安装，见[下载和安装](./install#ios) | iOS 16.4 及以上 |
| macOS | [NextTodo-macos-arm64.dmg](https://github.com/JunieXD/NextTodo/releases/latest/download/NextTodo-macos-arm64.dmg) | Apple 芯片，macOS 14 及以上 |
| macOS | [NextTodo-macos-x64.dmg](https://github.com/JunieXD/NextTodo/releases/latest/download/NextTodo-macos-x64.dmg) | Intel 芯片，macOS 14 及以上 |
| Windows | [NextTodo-windows-x64-setup.exe](https://github.com/JunieXD/NextTodo/releases/latest/download/NextTodo-windows-x64-setup.exe) | Windows 10 或 11，x64 |

链接下载的都是最新版。第一次打开时系统可能会拦下，处理办法见[下载和安装](./install)。

## 2. 选择用法 {#modes}

第一次打开 App 时，要在两种用法之间选一种：

| | 在本机使用 | 连接服务器 |
|---|---|---|
| 适合 | 先试用，或不想部署服务器 | 多台设备实时同步、使用网页版，或和别人共享文件夹 |
| 需要 | 只需安装 App | 一台自己的服务器；经公网访问时最好再配 HTTPS 证书 |
| 账号 | 不需要 | 用户名和密码 |
| 多台设备 | 可以借网盘同步，几十秒内到 | 实时同步 |
| 数据存放 | 这台设备上，可以备份和恢复；借网盘同步时网盘里也有一份 | 自己的服务器上，各设备各有一份副本 |

- **在本机使用**：点击“在本机使用”即可开始，适合先试用。几台设备之间可以借坚果云这类网盘同步。详见[在本机使用](./local)。
- **连接服务器**：先按[部署服务器](./server)准备好服务器，再点击“连接服务器”，填写服务器地址、用户名和密码登录。

两种用法的数据互不相通。先在本机使用、以后改用服务器也可以，本机的数据不会自动搬过去，可以用备份文件搬，见[搬到服务器和搬回本机](./local#move)。

## 3. 开始使用

1. 在添加框里写下一件事，按回车。添加框在手机屏幕底部，电脑上在标题下方。
2. 点开这一项，在“前置”里选出要先完成的事项。
3. 回到“接下来”，那里只列出现在能做的事。

详见[基本用法](./basics)。

## 4. 更多

- [共享文件夹](./share)：几个人分工做一个项目，各人只看自己要做的。
- [AI 整理](./ai)：把一大段话交给 AI，整理成带先后关系的事项。
- [接入 agent](./agent)：让 Claude Code、Codex 这类 AI 助手直接管理待办。
