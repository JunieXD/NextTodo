# agent 接入

agent 和脚本使用 NextTodo 的方式。产品层的定位见[总体设计](design.md#agent-接入)。

## 三个入口，一组工具

服务端实现一组工具，三个入口调用的是同一份实现，参数、行为和输出完全一致：

| 入口 | 适合 | 说明 |
|---|---|---|
| 命令行 `nexttodo` | 有终端的 agent（Claude Code、Codex）、脚本、人 | 每个子命令对应一个工具，只负责把参数发给服务端、打印结果 |
| MCP `/mcp` | 支持 MCP 的客户端 | Streamable HTTP，无状态 |
| 工具接口 `/api/v1/tools` | 其他程序 | 普通 HTTP，见[工具接口](#工具接口) |

工具的改动走同一条改动管线，和在 App 里操作一样实时同步到所有设备，记进改动记录，能撤销。

## 引用

- **事项**用编号：`12` 或 `#12`。
- **文件夹**用路径：`/工作/网站改版`，根目录是 `/`。输入时开头的 `/` 可以省略；文件夹名本身像数字时（如 `2026`）必须带 `/`，否则会被当成事项编号。
- **截止时间**写成 `2026-11-20` 或 `2026-11-20 18:00`。**提醒**写提前的分钟数，`0` 表示截止时提醒，只有带具体时间的截止才能设提醒。
- **批次**是改动记录里的序号，写操作的输出和 `log` 里都会给出，撤销时用它。

## 工具

| 工具 | 命令行 | 参数 | 作用 |
|---|---|---|---|
| `tree` | `tree` | | 文件夹树，每个文件夹直接包含的待办数和可做数 |
| `list` | `ls [文件夹] [--all] [--limit 条数]` | `folder?`、`all?`、`limit?` | 文件夹的子文件夹和事项，按[文件夹内顺序](design.md#排序)；默认不含已完成；文件夹里有 [AI 整理](ai.md)请求时开头注明 |
| `next` | `next [--limit 条数]` | `limit?` | 接下来：可做的事项，按[接下来的顺序](design.md#排序) |
| `show` | `show <事项>` | `task` | 事项详情：截止、提醒、前置、后续、来源、描述 |
| `search` | `search <关键词> [--limit 条数]` | `query`、`limit?` | 标题和描述里包含关键词的事项（不区分大小写） |
| `graph` | `graph [文件夹] [--all] [--mermaid]` | `folder?`、`all?`、`mermaid?` | 文件夹（含子文件夹）里的事项和依赖，按先后顺序；连到外面的事项也列出并注明文件夹 |
| `log` | `log [-n 条数]` | `limit?` | 最近的改动批次，默认 10，最多 100 |
| `jobs` | `jobs` | | [AI 整理](ai.md)的请求：等待、整理中和失败的，失败的带原因；整理完的请求已经变成事项，不在这里 |
| `add` | `add <标题> [--folder] [-d 描述] [--due] [--remind] [--after 编号…] [--before 编号…]` | `title`、`folder?`、`description?`、`due?`、`remind?`、`after?`、`before?` | 添加一个事项；文件夹不存在时一起新建 |
| `edit` | `edit <事项> [--title] [-d 描述 \| --replace 原文 新文… \| --append 文本] [--due \| --no-due] [--remind \| --no-remind]` | `task`、`title?`、`description?`、`replace?`、`append?`、`due?`、`remind?` | 修改事项；`due`、`remind` 给 `null` 表示清除；描述的改法见[同时修改](#同时修改) |
| `done` | `done <事项…>` | `tasks` | 标为完成，输出因此变成可做的事项 |
| `reopen` | `reopen <事项…>` | `tasks` | 重新打开 |
| `mkdir` | `mkdir <路径>` | `path` | 新建文件夹，路径上缺的层级一起建；已存在时报错 |
| `rename` | `rename <文件夹> <新名字>` | `folder`、`name` | 文件夹改名 |
| `move` | `mv <事项或文件夹…> <目标文件夹>` | `tasks?`、`folders?`、`to` | 移动到已有的文件夹，`/` 是根目录 |
| `delete` | `rm <事项或文件夹…>` | `tasks?`、`folders?` | 删除；文件夹连同里面的一切 |
| `dep_add` | `dep add <前置> <后续>` | `before`、`after` | 加依赖，成环时拒绝并列出环上的事项 |
| `dep_remove` | `dep rm <前置> <后续>` | `before`、`after` | 删依赖 |
| `apply` | `apply <文件或 -> [--dry-run]` | `plan`、`dry_run?` | 按[计划](#计划)批量新建文件夹、事项和依赖 |
| `undo` | `undo <批次>` | `batch` | 撤销一个批次，撤销本身也是一个新批次 |

- 命令行里 `mv`、`rm` 的参数像编号的是事项，其余是文件夹。
- `list`、`next`、`search` 最多列出 `limit` 项，默认 50，最多 500；没列完时最后一行说明还有多少项。`list` 先列未完成的，再列已完成的。
- `edit` 只改 `due` 不给 `remind` 时：新截止带具体时间就保留原来的提醒，否则去掉。
- 每个写操作是一个批次，全部生效或全部不生效。
- 只读令牌只能用前 8 个查询工具。

## 计划

`apply` 的输入，命令行里写成 YAML 文件，MCP 和工具接口里是同样结构的 JSON：

```yaml
folder: /工作/网站改版        # 可选，默认根目录；不存在就新建，缺的层级一起建
tasks:
  - ref: design              # 可选，计划内的临时名字，供 after、before 引用
    title: 出设计稿           # 必填
    due: 2026-11-20 18:00
    remind: 60
  - ref: api
    title: 写接口
    folder: 后端              # 可选，相对上面的 folder；以 / 开头是完整路径；不存在就新建
    description: |
      - [ ] 登录
      - [ ] 列表
  - title: 联调
    after: [design, api, 42]  # 前置：计划里的 ref，或已有事项的编号（42 或 "#42"）
    before: [43]              # 后续，写法同上
```

- 事项按列出的顺序排在各自文件夹的最后。
- `ref` 在计划内不能重复，也不能写成编号的样子（`42`、`#42`），免得和已有事项混淆；引用可以指向列表里靠后的事项。
- 一个计划最多产生 1000 个操作。
- `--dry-run`（`dry_run: true`）完整校验一遍并输出会发生的变化，但不保存。

## 同时修改

agent 和用户可能同时在改同一项：

- 每个写操作在服务端的一个事务里完成，编号、路径和描述都按那一刻的数据解析，全部生效或全部不生效。
- 修改只涉及给出的字段：agent 改截止时间、用户同时改标题，两边都保留。
- 改描述时用 `replace` 或 `append`，不要读出整段、改完再整段写回：
  - `replace` 是 `[{old, new}, …]`（命令行 `--replace 原文 新文`，可以重复），在服务端当时的描述上依次替换。`old` 必须恰好出现一次：找不到说明那里已被改过，报冲突，先 `show` 再改；出现多次时报错，多带一些上下文。
  - `append` 在描述末尾另起一行追加。
  - 两者可以同时给，先替换再追加。只发改动的那一段，用户同时改了描述的其他地方也不会被覆盖。
  - `description` 仍然整段替换，适合新写或重写，不能和前两者同时给。
- 撤销按字段进行：一个字段在那个批次之后又被改过，就保留现在的值，不会撤掉别人后来的修改。
- 用户在 App 里编辑标题或描述时，它在别处被改了的话，保存时 App 会提示，让用户选用哪一份，见[详情](design.md#详情)。

## 输出

默认输出简洁的中文文本，给人和 agent 读；命令行加 `--json` 输出结构化数据。

事项一行一个，依次是编号、状态、标题，后面用 ` · ` 接文件夹（在 `next`、`search` 和连到外面的事项上）、截止时间、解锁数：

```text
$ nexttodo ls 工作/网站改版
/工作/网站改版
  AI 整理：等待 1、失败 1（用 jobs 查看）
  后端/（待办 1，可做 1）
  #12 [可做] 出设计稿 · 截止 2026-11-20 18:00 · 解锁 1 项
  #16 [等 2 项] 联调

$ nexttodo next
#12 出设计稿 · /工作/网站改版 · 截止 2026-11-20 18:00 · 解锁 1 项
#15 写接口 · /工作/网站改版/后端

$ nexttodo graph 工作
#12 [可做] 出设计稿
#15 [可做] 写接口 · /工作/网站改版/后端
#16 [等 2 项] 联调 ← #12, #15
#20 [等 1 项] 上线 · /生活 ← #16
```

- 状态：`[可做]`、`[等 N 项]`（还有 N 个前置没完成）、`[已完成]`。“解锁 N 项”是做完它就能开始的后续数。
- 列表没列完时，最后一行是 `另有 N 项未列出，可调大 limit`。
- 列表里的标题是全文；写操作的结果、`log` 和 `show` 的前置、后续只是提到事项，标题超过 30 字时截成前 30 字加“…”，全文用 `show` 看。
- 写操作输出实际发生的变化，同类合并成一行，最后一行是批次：

  ```text
  新建文件夹 /工作/网站改版
  添加 3 项：#57 出设计稿、#58 写接口、#59 联调
  添加 3 条依赖：#57 → #59、#58 → #59、#42 → #59
  批次 128，可撤销
  ```

- 时间显示为“3 分钟前”“2 天前”，不涉及时区；截止时间按原样显示。
- 出错时输出服务端的错误说明，命令行退出码为 1。

`--json` 的数据：

| 工具 | 数据 |
|---|---|
| 事项 | `{id, number, title, status, folder}`；`status` 是 `ready`、`waiting`、`done`，`folder` 是路径；`waiting_on`（还在等的前置数）、`unlocks`（解锁数）不为 0 时才有，`due`、`done_at` 有值时才有 |
| `tree` | `[{path, name, depth, pending, ready}]`，先序排列，第一项是根目录 |
| `list` | `{folder, folders: [{path, name, pending, ready}], tasks: [事项], total}`，`total` 是没截断时的事项数；有 AI 整理请求时另有 `jobs: {waiting, running, failed}`，只含不为 0 的 |
| `next` | `{tasks: [事项], total}` |
| `show` | 事项加上 `created_at, updated_at`；`description`、`created_via`、`updated_via`（令牌名，本人时没有）、`before: [事项]`、`after: [事项]` 有内容时才有 |
| `search` | `{tasks: [事项], total}` |
| `graph` | `{tasks: [事项], edges: [{before, after}]}`，边用编号 |
| `log` | `[{batch, actor, created_at, summary}]`，`actor` 是令牌名，本人为 `null` |
| `jobs` | `[{id, start, folder, state, updated_at}]`，`start` 是原文开头，`state` 是 `waiting`、`running`、`failed`，失败的另有 `error` |
| 写操作 | `{batch, changes: [变化]}`，预览时 `batch` 为 `null`；完成事项后有事项因此可做时另有 `unlocked: [事项]` |

写操作的变化只说做了什么，不带事项的内容：

| `kind` | 其他字段 |
|---|---|
| `folder_created`、`folder_sorted`、`folder_deleted` | `folder`：路径 |
| `folder_renamed` | `folder`：原路径；`name`：新名字 |
| `folder_moved` | `folder`：原路径；`to`：新路径 |
| `task_created`、`task_done`、`task_reopened`、`task_deleted` | `task`：编号 |
| `task_moved` | `task`；`to`：文件夹路径 |
| `task_edited` | `task`；`fields`：改了的字段，`title`、`description`、`due`、`order`（顺序） |
| `dep_added`、`dep_removed` | `before`、`after`：编号 |
| `job_created`、`job_started`、`job_retried`、`job_deleted` | `job`：请求 ID |
| `job_failed` | `job`；`error`：原因 |

需要改动前后完整内容的程序用 [`/changes`](api.md#端点)。

## 命令行

```sh
cargo install --path crates/cli          # 安装 nexttodo
nexttodo login --url https://todo.example.com --name "Claude Code"
```

- `login` 用用户名和密码登录，生成一个以 `--name` 命名（默认“命令行”）的读写令牌，加 `--read-only` 生成只读令牌。令牌存在 `~/.config/nexttodo/config.json`（权限 600），登录用的会话随即注销。
- 环境变量 `NEXTTODO_URL`、`NEXTTODO_TOKEN` 优先于配置文件，可以给不同的 agent 用不同的令牌，改动记录里就能分清是谁做的。
- 网络出错时，写操作用同一个幂等键自动重试两次，不会重复生效。

## MCP

- 地址 `https://todo.example.com/mcp`，请求头 `Authorization: Bearer <令牌>`。
- 工具的结果是上面的文本；出错时返回 `isError` 和错误说明。
- 查询工具标注为只读，`delete` 标注为破坏性，客户端可以据此决定是否需要确认。

Claude Code：

```sh
claude mcp add --transport http nexttodo https://todo.example.com/mcp --header "Authorization: Bearer <令牌>"
```

Codex（`~/.codex/config.toml`）：

```toml
[mcp_servers.nexttodo]
url = "https://todo.example.com/mcp"
bearer_token_env_var = "NEXTTODO_TOKEN"
```

## 工具接口

- `GET /api/v1/tools`：工具列表，每项 `{name, description, input_schema}`，和 MCP 的 `tools/list` 相同。
- `POST /api/v1/tools/{name}`：请求体是参数 JSON，返回 `{text, data}`；读工具还返回 `seq`：结果包含到这个序号为止的全部改动，编号同 [`/changes`](api.md#端点)。错误格式同 [API](api.md#错误)。
- 写操作可以带请求头 `Idempotency-Key: <uuidv7>`，作为改动 ID：同一个键重发时原样返回第一次的结果（90 天内）；超过保留期，或者这个键是 App 提交的改动 ID 时，只返回批次号，不会再生效一次。

## Skill

`skills/nexttodo/SKILL.md` 教 agent 什么时候用、怎么用 NextTodo。把这个目录链接到 agent 的 skill 目录即可，如 `ln -s $PWD/skills/nexttodo ~/.claude/skills/nexttodo`。
