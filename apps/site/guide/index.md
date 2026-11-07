# 快速上手

## 1. 安装 App

到 [GitHub Releases](https://github.com/JunieXD/NextTodo/releases/latest) 下载对应平台的安装包：

| 平台 | 文件 | 要求 |
|---|---|---|
| Android | `NextTodo-<版本>-android.apk` | Android 8.0 及以上 |
| macOS | `NextTodo-<版本>-macos-arm64.dmg` | Apple 芯片，macOS 14 及以上 |
| Windows | `NextTodo-<版本>-windows-x64-setup.exe` | Windows 10 或 11，x64 |

macOS 和 Windows 的安装包没有经过商业签名，首次打开时系统会提示：

- **macOS**：首次打开会被拦下。到“系统设置 → 隐私与安全性”中点击“仍要打开”。
- **Windows**：出现 SmartScreen 提示时，点击“更多信息 → 仍要运行”。

## 2. 选择用法

App 的登录页提供两种用法，两者的数据互不相通：

| | 在本机使用 | 连接服务器 |
|---|---|---|
| 适合 | 只在一台设备上使用 | 在多台设备之间同步，或使用网页端 |
| 需要 | 只需安装 App | 一台服务器和一个域名 |
| 账号 | 不需要 | 用户名和密码 |
| 数据存放 | 这台设备上，可以备份和恢复 | 自己的服务器上，各设备各有一份副本 |

- **在本机使用**：点击“在本机使用”即可开始。
- **连接服务器**：先部署服务端（见下一节），再在登录页的“服务器地址”里填写域名，用用户名和密码登录。之后 App 从这台服务器获取更新，不需要再手动下载。

## 3. 部署服务端

只在连接服务器时需要。服务端和网页端在同一个 Docker 镜像里，支持 amd64 和 arm64。在服务器上运行：

```sh
mkdir nexttodo && cd nexttodo
curl -fsSLO https://raw.githubusercontent.com/JunieXD/NextTodo/main/deploy/compose.yaml
mkdir data && sudo chown 65534:65534 data
docker compose up -d
docker compose exec nexttodo /nexttodo-server create-admin <用户名>
```

最后一条命令创建管理员账号，按提示输入两次密码。服务端默认监听 `127.0.0.1:8080`，前面需要一个提供 HTTPS 的反向代理。反向代理的配置、升级和备份见[部署文档](https://github.com/JunieXD/NextTodo/blob/main/docs/deploy.md)。

其他人注册需要邀请码，由管理员在设置的“邀请注册”里生成，也可以复制邀请链接发给对方。部署好后，用浏览器打开域名就是网页端，可以安装为 PWA。

## 4. 接入 agent

在 App 的设置里生成令牌，页面上会给出填好地址和令牌的命令行、Claude Code 和 Codex 配置，复制即可。命令行的下载和安装见 [agent 接入](https://github.com/JunieXD/NextTodo/blob/main/docs/agent.md#命令行)。

本机模式下，agent 只能在电脑上接入，并且要在 App 运行时使用。
