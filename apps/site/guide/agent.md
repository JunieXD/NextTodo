# 接入 agent

agent 指 Claude Code、Codex 这类能在电脑上执行命令的 AI 助手。接入 NextTodo 后，可以让它拆分计划、添加事项、设置先后、勾选完成，就像在 App 里操作一样，改动会同步到所有设备。

agent 有两种接入方式，任选其一：

- **MCP**：agent 直接连 NextTodo，配置一次即可。
- **命令行**：先装好命令行 `nexttodo`，agent 通过它读写待办；再装上 skill，agent 就知道怎么用。

## 1. 生成令牌

令牌是给 agent 用的通行证，每个 agent 用一个，方便分清谁做了什么。

1. 打开“设置 → agent 接入”，在“令牌”一组点“生成令牌”。
2. 名称写 agent 的名字，比如“Claude Code”，它会显示在这个 agent 做的改动旁边。
3. 选“读写”或“只读”，点“生成”。

令牌只显示这一次。页面上同时给出已经填好服务器地址和令牌的 Claude Code、Codex 配置和命令行环境变量，点右侧的按钮复制即可。不再使用时，在“设置 → agent 接入”里删除它，用这个令牌的 agent 随即失去访问权限。

## 2. 安装命令行 {#cli}

用 MCP 连接服务器的，可以跳过这一步。

**用桌面 App 安装（macOS 和 Windows）**：

1. 打开“设置 → agent 接入”，在“命令行”一组点“安装”。
2. macOS 上可能会弹出系统的密码框，输入登录密码确认。
3. 新打开一个终端窗口，运行 `nexttodo --version`，显示的版本应和“设置 → 关于”里的 App 版本相同。

版本不同，说明以前用别的方式装过命令行，终端用的是那一个。查看终端实际用的是哪个：macOS 上运行 `which -a nexttodo`，Windows 上运行 `where.exe nexttodo`（PowerShell 和命令提示符都适用），第一行就是。确认它是以前自己装的 NextTodo 命令行（比如用 `cargo install` 或安装脚本装的）后，按原来的方式卸载或删掉它；macOS 上也可以在 shell 配置文件里把 `/usr/local/bin` 排到它所在的目录前面。

App 更新时，命令行会一起更新。macOS 上要先把 App 移到“应用程序”文件夹，见[下载和安装](./install#macos)。

**用安装脚本（macOS 和 Linux）**：在终端里运行：

```sh
curl -fsSL https://juniexd.github.io/NextTodo/install.sh | sh
```

脚本把最新版装进 `~/.local/bin`。这个目录不在 `PATH` 里时，脚本会给出要加进 shell 配置文件的一行。以后服务器升级了，再运行一次即可更新。

**手动下载**：从 [GitHub Releases](https://github.com/JunieXD/NextTodo/releases/latest) 下载对应平台的文件，改名为 `nexttodo`（Windows 上是 `nexttodo.exe`），放进 `PATH` 里的目录。

## 3. 连接

### 连接服务器时

- **Claude Code**：复制令牌页上 Claude Code 的那条命令，在终端里运行。
- **Codex**：把令牌页上 Codex 的配置加进 `~/.codex/config.toml`，再按页面上的写法设置环境变量。
- **命令行**：登录后，把 skill 装给 agent：

  ```sh
  nexttodo login --url https://todo.example.com --name "Claude Code"
  nexttodo skill install
  nexttodo next
  ```

  `login` 会要求输入用户名和密码，并为这台电脑生成一个令牌；`next` 列出现在能做的事，能看到就说明连上了。

### 在本机使用时

App 在本机运行时，每次启动的地址都不同，所以 agent 要经过命令行连接：

1. 先[安装命令行](#cli)。
2. 生成令牌后，按令牌页给出的写法配置 Claude Code 或 Codex，或者设置环境变量 `NEXTTODO_URL=local` 和 `NEXTTODO_TOKEN`。

只在电脑上、NextTodo 运行时可用（关掉窗口也算在运行）。Android 的本机模式不能接入 agent。

## 撤销 agent 的改动

agent 改完后，App 的提示条会写明它做了什么，比如“Claude Code 在‘求职’中添加了 4 项”，点“撤销”即可撤回。

每次改动都有记录。错过了提示条，可以让 agent 运行 `nexttodo log` 查看最近的改动，再用 `nexttodo undo <批次>` 撤销其中一次。

更完整的说明（工具列表、计划的格式）见开发文档 [agent 接入](https://github.com/JunieXD/NextTodo/blob/main/docs/agent.md)。
