<div align="center">
  <img src="apps/web/public/icon.svg" width="88" alt="">
  <h1>NextTodo</h1>
  <p><strong>理清先后，专注当下</strong></p>
  <p>以先后关系为核心的待办应用，可在本机使用，也可自行部署、多端同步</p>
  <p>
    <a href="https://github.com/JunieXD/NextTodo/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/JunieXD/NextTodo?label=%E7%89%88%E6%9C%AC&color=2E8E6C"></a>
    <a href="LICENSE"><img alt="MIT 许可证" src="https://img.shields.io/badge/%E8%AE%B8%E5%8F%AF%E8%AF%81-MIT-2E8E6C"></a>
    <img alt="网页、Android、macOS、Windows" src="https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-%E7%BD%91%E9%A1%B5%20%C2%B7%20Android%20%C2%B7%20macOS%20%C2%B7%20Windows-2E8E6C">
  </p>
</div>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/desktop-dark.webp">
  <img src="docs/images/desktop-light.webp" alt="电脑上的“接下来”和事项详情">
</picture>

## 简介

事项之间可以设置依赖，组成一张有向无环图。NextTodo 据此算出每个事项的状态：前置都已完成的可以开始，其余的等待前置。首页随时列出现在能做的事，完成一项，它解锁的后续随即出现。

App 可以只在本机使用，不需要服务器和账号；也可以连接自己部署的服务器，网页、Android、macOS 和 Windows 实时同步，离线时照常使用。agent 可以通过命令行和 MCP 管理全部待办，每次改动都有记录，可以撤销。

## 功能

- **接下来**：汇总所有可以开始的事项，按截止时间分组；每项标出完成后能解锁几项。
- **依赖**：为事项添加前置和后续，形成环的依赖会被拒绝。前置未完成的事项显示为等待中，不进入“接下来”。文件夹列表按先后排列，左侧的轨道画出依赖。
- **关系图**：以图的形式查看一个文件夹里的先后关系，可以直接连线添加依赖。电脑和手机各有一套适合的交互。
- **文件夹**：可以无限嵌套，用来分开不同的项目。
- **截止和提醒**：截止时间可以精确到分钟，Android 和桌面 App 到点弹出通知。
- **离线可用**：操作在本地立即生效，联网后同步到所有设备。
- **AI 整理**：把随手写的一大段话交给自己配置的大模型（OpenAI 兼容接口，预设 DeepSeek、通义千问、OpenAI，也可以填其他兼容的地址），在后台整理成带依赖的事项，原文的信息不丢。也可以整理一个文件夹里已有的事项：改写标题和描述、补上先后关系，逐条确认后再应用。配置可以只存在本机，也可以加密保存在服务器上。
- **agent 接入**：命令行、MCP 和 HTTP 接口使用同一组工具，支持一次提交整份计划；令牌分只读和读写。
- **本机模式**：macOS、Windows 和 Android App 可以不连服务器，数据只存在这台设备上，可以备份和恢复。
- **多账号**：管理员生成邀请码，其他人凭邀请码注册，数据彼此独立。
- **Android 小组件**：在桌面查看“接下来”，直接勾选完成或快速添加。
- **快速添加**：桌面 App 可以用全局快捷键或菜单栏、托盘图标随时呼出。
- **自动更新**：App 从你的服务器获取更新，版本始终与服务端一致；本机模式从 GitHub Releases 获取。

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/graph-dark.webp">
  <img src="docs/images/graph-light.webp" alt="关系图">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/phones-dark.webp">
  <img src="docs/images/phones-light.webp" alt="手机上的“接下来”、文件夹和关系图">
</picture>

## 开始使用

### 1. 安装 App

| 平台 | 获取方式 | 要求 |
|---|---|---|
| Android | [下载](https://github.com/JunieXD/NextTodo/releases/latest) `NextTodo-<版本>-android.apk` | Android 8.0 及以上 |
| macOS | [下载](https://github.com/JunieXD/NextTodo/releases/latest) `NextTodo-<版本>-macos-arm64.dmg` | Apple 芯片，macOS 14 及以上 |
| Windows | [下载](https://github.com/JunieXD/NextTodo/releases/latest) `NextTodo-<版本>-windows-x64-setup.exe` | Windows 10 或 11，x64 |
| 网页 | 用浏览器打开服务器的域名，可以安装为 PWA | 需要先部署服务端 |

macOS 和 Windows 的安装包没有经过商业签名。macOS 首次打开会被拦下，到“系统设置 → 隐私与安全性”中点击“仍要打开”；Windows 出现 SmartScreen 提示时，点击“更多信息 → 仍要运行”。

### 2. 选择用法

App 的登录页提供两种用法，两者的数据互不相通：

- **在本机使用**：点击“在本机使用”即可，不需要服务器和账号。数据只存在这台设备上，不和其他设备同步，可以在设置里备份和恢复。详见[本机模式](docs/local.md)。
- **连接服务器**：需要多台设备同步或使用网页端时，先按下一节部署服务端，再在“服务器地址”里填你的域名，用用户名和密码登录。之后的更新从这台服务器获取，不需要再手动下载。

### 3. 部署服务端（连接服务器时）

服务端和网页端在同一个 Docker 镜像里，支持 amd64 和 arm64。在服务器上：

```sh
mkdir nexttodo && cd nexttodo
curl -fsSLO https://raw.githubusercontent.com/JunieXD/NextTodo/main/deploy/compose.yaml
mkdir data && sudo chown 65534:65534 data
docker compose up -d
docker compose exec nexttodo /nexttodo-server create-admin <用户名>
```

服务端默认监听 `127.0.0.1:8080`，前面需要一个提供 HTTPS 的反向代理。反向代理的配置、升级和备份见[部署文档](docs/deploy.md)。其他人凭你在设置里生成的邀请码注册。

### 4. 接入 agent

- **命令行**：从 [Releases](https://github.com/JunieXD/NextTodo/releases/latest) 下载 `nexttodo`（macOS、Linux、Windows），登录后把 skill 装给 agent：

  ```sh
  nexttodo login --url https://todo.example.com --name "Claude Code"
  nexttodo skill install
  nexttodo next
  ```

- **MCP**：在 App 的设置里生成令牌，页面上会给出填好地址和令牌的 Claude Code、Codex 配置，复制即可。
- **本机模式**：在电脑上使用，App 运行时可用。先装好命令行，再按令牌页给出的写法设置；MCP 经 `nexttodo mcp` 转接。Android 的本机模式不支持接入 agent。

下载和安装的细节、工具列表、计划格式见 [agent 接入](docs/agent.md)。

## 开发

| 做什么 | 命令 |
|---|---|
| 跑全部 Rust 检查 | `cargo fmt --check && cargo clippy --all-targets && cargo test` |
| 启动开发版服务端（`127.0.0.1:8080`） | `cargo run -p nexttodo-server -- --data .dev-data serve` |
| 创建管理员 | `cargo run -p nexttodo-server -- --data .dev-data create-admin <用户名>` |
| 网页端开发（`127.0.0.1:5173`，转发到开发版服务端） | `cd apps/web && pnpm install && pnpm dev` |
| 网页端检查 | `cd apps/web && pnpm check` |
| 安装命令行 `nexttodo` | `cargo install --path crates/cli` |
| 命令行连开发版 | `nexttodo login --url http://127.0.0.1:8080` |
| 构建发布（服务端镜像内容和各端安装包） | `deploy/build.sh` |

Android 和桌面 App 的构建见 [Android](docs/android.md#构建和签名) 和[桌面 App](docs/desktop.md#构建)，发布流程见[部署](docs/deploy.md#构建发布)。参与开发前请先读 [AGENTS.md](AGENTS.md)。

## 文档

| 文档 | 内容 |
|---|---|
| [总体设计](docs/design.md) | 定位、概念和规则、同步、技术栈、部署和更新 |
| [界面](docs/ui.md) | 布局、交互和视觉规范 |
| [数据模型](docs/data-model.md) | 数据库的表和规则 |
| [API](docs/api.md) | REST 接口和同步协议 |
| [agent 接入](docs/agent.md) | 命令行、MCP 和工具接口 |
| [AI 整理](docs/ai.md) | 整理成事项、整理文件夹、模型配置 |
| [本机模式](docs/local.md) | 不连服务器时的做法、备份和恢复 |
| [网页端](docs/web.md) | 前端结构、本地数据和同步 |
| [Android](docs/android.md) | Android App 和小组件 |
| [桌面 App](docs/desktop.md) | macOS 和 Windows App |
| [部署](docs/deploy.md) | 自己部署、升级、备份和构建发布 |

## 许可证

[MIT](LICENSE)
