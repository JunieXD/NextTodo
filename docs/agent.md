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

- **事项**用编号：`12` 或 `#12`。别人[共享](design.md#共享)给我的文件夹里的事项用共享人的编号，前面加上共享人的用户名：`@alice#42`。
- **文件夹**用路径：`/工作/网站改版`，根目录是 `/`。输入时开头的 `/` 可以省略；文件夹名本身像数字时（如 `2026`）必须带 `/`，否则会被当成事项编号。别人共享给我的文件夹从共享人的用户名开始：`@alice/装修`、`@alice/装修/水电`。
- **人**用用户名：`bob`。
- YAML 计划里以 `@` 开头的值要加引号（`"@alice#42"`）。
- **开始时间**写法同截止时间（农历的也一样）：`2026-11-14` 或 `2026-11-14 09:30`，没到时事项等着，不能晚于截止，见[总体设计](design.md#开始时间)。
- **截止时间**写成 `2026-11-20` 或 `2026-11-20 18:00`。按[农历](design.md#农历)说的写 `农历八月十五`、`农历腊月廿三 18:00`，是今天（`today`）起最近的那一天（今天的几点已过的算下一次，“现在”同开始时间），存下的是换算出的公历日期；月份也可以写数字（`农历8月15`），闰月写 `农历闰六月初一`；小月没有三十，取最后一天，所以除夕写 `农历腊月三十`；不写月份的 `农历初一` 是今天起的第一个初一。配上按农历每个月几号的重复（`1lm:1,15`）时，从写的那个月起（不写月份或写的是本月的，从现在起）、不早于开始时间，取这几天里最先到的那天；要定下第一次的，截止时间写公历日期。**提醒**可以有多个：提前多久写 `"30m"`、`"2h"`、`"1d"`、`"1w"`（提前 30 分钟、2 小时、1 天、1 周，可以组合，如 `"2h30m"`）或分钟数，`0` 是截止时，要有带具体时间的截止（只有日期的截止上，整天的 `"1d"`、`"1w"`、`0` 是提前那么多天的 09:00，同 App 里自定义提醒的默认时间）；`"1d 20:00"` 是提前 1 天的 20:00（也可以按周，`"1w 09:00"`），`"09:00"`（即 `"0d 09:00"`）是截止那天的 9 点，只有日期的截止也能设。一个直接写，几个写成列表，如 `remind: [60, "1d 20:00"]`，也可以写在一个字符串里用逗号隔开，命令行写 `--remind 60 "1d 20:00"`。这些写法照着模型自然会写的定，见 [AI 整理](ai.md#提示词和结果)。`show` 里写成“提醒：前一天 20:00、提前 1 小时”。见[总体设计](design.md#提醒)。
- **前置完成后多久以内截止**写成 `due_after: 30d`（天）、`2w`（周）或 `3d 18:00`，代替 `due`：前置都完成的那天起算，`0d` 是当天；没有前置、或前置已经都完成的，从今天算。“拿到发票后 30 天内报销”就是前置“拿到发票”加 `due_after: 30d`；从今天算的“三天后”照样写成 `due` 的日期。提醒照样给，不能和重复一起用。还没定下时 `show` 和列表写“截止：前置完成后 30 天内”，定下以后写日期，后面注明“（前置完成后 30 天内）”；数据里另有 `due_after`。见[总体设计](design.md#前置完成后的截止)。
- **重复**写成间隔数加单位：`1d`、`2w`、`3m`、`1y`（每天、每两周、每三个月、每年；`1y` 就是 `12m`）；按周的可以在冒号后写周几（`1` 是周一，`7` 是周日），如 `1w:1,2,3,4,5`（每个工作日，即周一到周五）、`1w:1,3,5`（每周一三五）、`2w:2,4`（每两周的周二和周四）；每个法定工作日（周一到周五，跳过放假、算上补班，见[总体设计](design.md#重复)）写 `workdays`，用户说“每个工作日”的一般就是它；按月的可以在冒号后写几号（1–31，没有这天的月份取最后一天），如 `1m:1,15`（每月 1 号和 15 号）、`1m:31`（每月最后一天）。按[农历](design.md#重复)的在单位前加 `l`，可以在冒号后写几号（1–30，初一到三十），不写的就是截止日期的农历日子（截止日期写成农历的按写的那天，所以 `农历腊月三十` 加 `1ly` 是每年除夕，腊月小的年份是廿九），如 `1ly`（每年农历的这一天）、`1lm:1,15`（农历每月初一、十五）、`1ly:15`（每年农历的十五，在截止日期的那个农历月）；截止日期直接写成农历日期最省事（`due: 农历八月十五`、`repeat: 1ly`），`show` 和列表在这样的截止日期后面写出农历，如 `2027-09-15（农历八月十五）`。只有带截止时间的事项能设；`repeat_from` 是 `due`（按截止日期，默认）或 `done`（从完成那天）；`repeat_until` 是重复到哪天（`2026-12-19`，含这天），不给就一直重复，做完最后一次事项就完成了。规则见[总体设计](design.md#重复)。
- **今天和现在**：所有工具都可以带 `today` 和 `time`，用户当地的日期（`YYYY-MM-DD`）和几点几分（`HH:MM`），用来判断开始时间到了没有；重复的事项完成时按 `today` 顺延。命令行自动带上本机的日期和时间。只带 `today` 的，那天开始的都算已经到了；都不带的按 UTC 的现在算，完成重复的事项时必须带 `today`。`time` 要和 `today` 一起给。
- **批次**是改动记录里的序号，写操作的输出和 `log` 里都会给出，撤销时用它。

## 工具

| 工具 | 命令行 | 参数 | 作用 |
|---|---|---|---|
| `tree` | `tree` | | 文件夹树，每个文件夹里（含子文件夹）的待办数和可做数；共享的文件夹注明成员，别人共享给我的列在后面 |
| `list` | `ls [文件夹] [--all] [--limit 条数]` | `folder?`、`all?`、`limit?` | 文件夹的子文件夹和事项，按[文件夹内顺序](design.md#排序)；默认不含已完成；文件夹里有 [AI 整理](ai.md)请求时开头注明；在共享的文件夹里时开头写共享人和成员 |
| `next` | `next [--folder 路径…] [--limit 条数]` | `folders?`、`limit?` | 接下来：[我的事项](design.md#共享)里可做的，按[接下来的顺序](design.md#排序)；给了 `folders` 只列这些文件夹（含子文件夹）里的，有 `/` 时就是全部 |
| `agenda` | `agenda [--days 天数] [--folder 路径…]` | `days?`、`folders?` | 日程：今天起几天（默认 7，最多 90）每天截止的[我的事项](design.md#共享)，[重复](design.md#重复)事项以后的每一次也算，同[日历](design.md#日历)；前面另列截止日期在今天以前、还没完成的；也写出这几天的[节日、节气和放假、补班](design.md#节日和放假)，写在日期后面，如“2027-02-05（明天，除夕，放假）：”，没有事项的日子只有这一行；`folders` 同 `next` |
| `show` | `show <事项>` | `task` | 事项详情：负责人、开始、截止、提醒、完成次数和最近的完成记录（重复的事项）、前置、后续、附件（ID、名称、类型、大小）、来源、描述 |
| `search` | `search <关键词> [--limit 条数]` | `query`、`limit?` | 标题和描述里包含关键词的事项（不区分大小写） |
| `graph` | `graph [文件夹] [--all] [--mermaid]` | `folder?`、`all?`、`mermaid?` | 文件夹（含子文件夹）里的事项和依赖，按先后顺序；连到外面的事项也列出并注明文件夹 |
| `log` | `log [批次] [-n 条数] [--before 批次] [--deleted]` | `batch?`、`limit?`、`before?`、`deleted?` | 最近的改动批次，默认 10，最多 100；包括别的成员在共享的文件夹里做的，写明是谁。每类变化最多写 3 个事项。`before` 只列这个批次之前的，用来往前翻；`deleted` 只列删除了事项、文件夹或附件的；给了 `batch` 只看这一个批次，逐样列出改了什么，每样下面写改了的字段，改前和改后（添加、删除的写当时有值的字段；描述只写开头，全文在 `--json` 里），其他参数不起作用 |
| `jobs` | `jobs` | | [AI 整理](ai.md)的请求：等待、整理中和失败的，失败的带原因，以及建议待用户确认的整理文件夹；整理完的请求已经变成事项，不在这里 |
| `attachment` | `download <附件 ID> [-o 路径]` | `id` | 读一个[附件](design.md#附件)：PNG、JPEG、WebP、GIF 图片（5 MB 以内）在 MCP 里作为图片返回，agent 能直接看；UTF-8 的文本文件（`text/` 开头的和 JSON，1 MB 以内）返回内容；其他文件只给名称、类型和大小。命令行的 `download` 不经过这个工具，直接下载文件，存到 `-o` 给的文件或目录，不给时存到当前目录；存到目录时用附件的名称（按[桌面 App](desktop.md#附件)的规则改成能用的文件名）。已有同名文件时报错，不覆盖 |
| `add` | `add <标题> [--folder] [-d 描述] [--start] [--due \| --due-after] [--remind] [--repeat [--repeat-from] [--repeat-until]] [--assign 用户名…] [--after 编号…] [--before 编号…]` | `title`、`folder?`、`description?`、`start?`、`due?`、`due_after?`、`remind?`、`repeat?`、`repeat_from?`、`repeat_until?`、`assign?`、`after?`、`before?` | 添加一个事项；文件夹不存在时一起新建 |
| `edit` | `edit <事项> [--title] [-d 描述 \| --replace 原文 新文… \| --append 文本] [--start \| --no-start] [--due \| --due-after \| --no-due] [--remind \| --no-remind] [--repeat [--repeat-from] [--repeat-until] \| --no-repeat] [--assign 用户名…] [--unassign 用户名…]` | `task`、`title?`、`description?`、`replace?`、`append?`、`start?`、`due?`、`due_after?`、`remind?`、`repeat?`、`repeat_from?`、`repeat_until?`、`assign?`、`unassign?` | 修改事项；`due`、`due_after`、`remind`、`repeat` 给 `null` 表示清除；描述的改法见[同时修改](#同时修改)；负责人在现有的上面加、减 |
| `done` | `done <事项…> [--end-repeat]` | `tasks`、`today?`、`end_repeat?` | 标为完成，输出因此变成可做的事项；重复的事项改为顺延，输出下一次的截止时间；做的是 `repeat_until` 前的最后一次的，直接完成。有重复的事项时必须给 `today`（用户当地的当天日期），命令行自动带上本机日期。`end_repeat` 让重复的事项结束重复、直接完成，用于打卡的长期事项全部做完时，见[总体设计](design.md#重复) |
| `skip` | `skip <事项…>` | `tasks`、`today` | 跳过重复事项的这一次：和完成一样顺延到下一次，但不算完成，见[总体设计](design.md#重复)。已是最后一次的报错，用 `done --end-repeat`；不重复的事项报错。命令行自动带上本机日期 |
| `drop` | `drop <事项…> [--with-after]` | `tasks`、`with_after?` | [放弃](design.md#放弃)：不做了，和完成一样不再挡着后续，输出因此变成可做的事项；重复的事项整个结束，只是这次不做用 `skip`。`with_after` 连同还没结束的后续（后续的后续也算）一起放弃。已经结束的报错 |
| `reopen` | `reopen <事项…>` | `tasks` | 重新打开已完成或放弃的事项 |
| `mkdir` | `mkdir <路径>` | `path` | 新建文件夹，路径上缺的层级一起建；已存在时报错 |
| `rename` | `rename <文件夹> <新名字>`、`rename --attachment <附件 ID> <新名字>` | `folder?`、`attachment?`、`name` | 文件夹或附件改名，`folder` 和 `attachment` 正好给一个 |
| `move` | `mv <事项或文件夹…> <目标文件夹>` | `tasks?`、`folders?`、`to` | 移动到已有的文件夹，`/` 是根目录；移进、移出共享的文件夹时去掉连到外面的依赖和看不到的负责人，并换成新主人的编号，输出里写明 |
| `style` | `style <文件夹> [--color 颜色 \| --no-color] [--icon 图标 \| --no-icon]` | `folder`、`color?`、`icon?` | 设文件夹的[颜色和图标](ui.md#文件夹的颜色和图标)，`null`（`--no-color`、`--no-icon`）改回默认；颜色和图标的取值见[数据模型](data-model.md#folders) |
| `copy` | `cp <文件夹> [目标文件夹] [--name 名字]` | `folder`、`to?`、`name?` | [复制文件夹](design.md#文件夹)，连同子文件夹、事项、附件和它们之间的依赖，事项都是待办；放在目标文件夹的最后，不给 `to` 就在原文件夹的同一层（别人共享给我的文件夹在根目录）；不给 `name` 时用原名，那里已有同名的就叫“原名 副本”“原名 副本 2”…… |
| `delete` | `rm <事项或文件夹…>` | `tasks?`、`folders?` | 删除；文件夹连同里面的一切 |
| `dep_add` | `dep add <前置> <后续>` | `before`、`after` | 加依赖，成环时拒绝并列出环上的事项 |
| `dep_remove` | `dep rm <前置> <后续>` | `before`、`after` | 删依赖 |
| `attach` | `attach <事项> <文件…> [--name 名称]` | `task`、`name`、`data`、`type?` | 给事项加一个附件：`data` 是文件内容的 base64，最多 20 MB；`type` 不给时按扩展名推断，推断不出是 `application/octet-stream`。命令行每个文件一个批次，`--name` 只能和一个文件一起给。原样保存，不压缩 |
| `detach` | `detach <附件 ID…>` | `attachments` | 删除附件，可撤销 |
| `apply` | `apply <文件或 -> [--dry-run]` | `plan`、`dry_run?` | 按[计划](#计划)批量新建文件夹、事项和依赖 |
| `share` | `share <文件夹> <用户名…>` | `folder`、`users` | 共享人把文件夹共享给这些人，或添加成员 |
| `unshare` | `unshare <文件夹> [用户名…]` | `folder`、`users?` | 共享人移除这些成员，不写就是停止共享；成员不写用户名就是自己退出 |
| `undo` | `undo <批次>` | `batch` | 撤销一个批次，撤销本身也是一个新批次；看得到全部改动、也有权做的批次才能撤销，别的成员的也行 |

- 命令行里 `mv`、`rm` 的参数像编号的是事项，其余是文件夹。
- `list`、`next`、`search` 最多列出 `limit` 项，默认 50，最多 500；没列完时最后一行说明还有多少项。`list` 先列未完成的，再列已完成的。
- `edit` 的 `start` 给 `null`（命令行 `--no-start`）清除开始时间，`due` 同样；只写日期的 `start` 去掉原来的几点。改了截止或开始，开始晚于截止时报错。
- `edit` 给了 `due` 就不再是前置完成后的，给了 `due_after` 就不再按日期；两个不能一起给。只改 `remind` 时前置完成后的照旧。
- `edit` 给了 `remind` 就整个换掉原来的提醒。只改 `due` 不给 `remind` 时提醒保留，新截止不带具体时间的去掉提前多少分钟的；重复总是保留，清除截止时间时一起清除；新截止晚于 `repeat_until` 时报错。`repeat_from`、`repeat_until` 只能和 `repeat` 一起给：给了 `repeat` 就整个换掉，没给的按默认。
- 负责人只能是共享人和成员，只有共享的文件夹里的事项能设。`assign`、`unassign` 在服务端当时的负责人上加、减，两个人同时设也不会互相覆盖。
- 依赖只能连在同一处：私人的事项之间，或同一个共享的文件夹里的事项之间，见[依赖](design.md#依赖)。
- 每个写操作是一个批次，全部生效或全部不生效；重发不会重复生效，见[重试](#重试)。
- 只读令牌只能用前 9 个查询工具。

## 计划

`apply` 的输入，命令行里写成 YAML 文件，MCP 和工具接口里是同样结构的 JSON：

```yaml
folder: /工作/网站改版        # 可选，默认根目录；不存在就新建，缺的层级一起建
tasks:
  - ref: design              # 可选，计划内的临时名字，供 after、before 引用
    title: 出设计稿           # 必填
    due: 2026-11-20 18:00
    remind: 60
    repeat: 1w                # 可选，写法同 add；repeat_from、repeat_until 也可以写
  - ref: review
    title: 根据评审意见改稿
    after: [design]
    due_after: 3d             # 可选，代替 due：前置都完成后 3 天内
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
- `ref` 在计划内不能重复，也不能写成编号的样子（`42`、`#42`、`@alice#42`），免得和已有事项混淆；引用可以指向列表里靠后的事项。
- 共享的文件夹里的事项可以写负责人 `assign: [bob, carol]`，规则同 `add`。
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

事项一行一个，依次是编号、状态（`next` 里都可做，不写；`agenda` 里重复事项以后的一次也不写）、标题，后面用 ` · ` 接文件夹（在 `next`、`agenda`、`search` 和连到外面的事项上）、负责人（“负责 bob、carol”）、截止时间、[关联进度](design.md#关联进度)（在 `next` 上）、解锁数、要赶的[后续的截止](design.md#后续的截止)（在 `next` 上，“后续 #14 交报告 2026-11-27 截止”，设了截止但晚于后续的是“截止晚于后续 #14 交报告（2026-11-27）”）：

```text
$ nexttodo ls 工作/网站改版
/工作/网站改版
  AI 整理：等待 1、失败 1（用 jobs 查看）
  后端/（待办 1，可做 1）
  #12 [可做] 出设计稿 · 截止 2026-11-20 18:00 · 解锁 1 项
  #16 [等 2 项] 联调

$ nexttodo next
#12 出设计稿 · /工作/网站改版 · 截止 2026-11-20 18:00 · 第 1/3 步 · 解锁 1 项
#15 写接口 · /工作/网站改版/后端 · 第 1/3 步

$ nexttodo graph 工作
#12 [可做] 出设计稿
#15 [可做] 写接口 · /工作/网站改版/后端
#16 [等 2 项] 联调 ← #12, #15
#20 [等 1 项] 上线 · /生活 ← #16
```

- 状态：`[可做]`、`[等 N 项]`（还有 N 个前置没完成）、`[2026-11-14 09:30 开始]`（还没到开始时间，没设几点的写 `[2026-11-14 开始]`；两样都等的是 `[等 N 项，2026-11-14 开始]`）、`[已完成]`。“第 2/5 步”是沿着先后关系，这一项是第 2 步、共 5 步。“解锁 N 项”是做完它就能开始的后续数。
- 列表没列完时，最后一行是 `另有 N 项未列出，可调大 limit`。
- 别人共享给我的文件夹里的事项，编号写成 `@alice#42`，文件夹写成 `@alice/装修`，和输入时一样。
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
| 事项 | `{id, number, title, status, folder}`；`status` 是 `ready`、`waiting`、`done`、`dropped`，`folder` 是路径；别人共享给我的文件夹里的事项另有 `owner`（共享人的用户名，引用写成 `@owner#number`），有负责人时另有 `assignees`（用户名）；`waiting_on`（还在等的前置数）、`unlocks`（解锁数）不为 0 时才有，`start`、`due`、`due_after`（同 API 的写法 `{days, time, reminders}`）、`done_at`、`completions`（`{count, last}`，重复的事项完成过时）有值时才有 |
| `tree` | `[{path, name, depth, pending, ready}]`，先序排列，第一项是根目录，别人共享给我的文件夹在最后；共享的文件夹另有 `owner` 和 `members`（用户名），设了颜色或图标的另有 `color`、`icon` |
| `list` | `{folder, folders: [{path, name, pending, ready}], tasks: [事项], total}`，`total` 是没截断时的事项数；有 AI 整理请求时另有 `jobs: {waiting, running, failed, ready}`，只含不为 0 的；在共享的文件夹里时另有 `owner` 和 `members` |
| `next` | `{tasks: [事项], total}`；有前置或后续的事项另有 `step: [2, 5]`（第几步、共几步）；要赶[后续的截止](design.md#后续的截止)的另有 `due_by: {date, time?, number, title}`（那一项的截止和编号、标题）；我的事项里有还没到开始时间的，或者有后续没结束的[重复事项](design.md#重复)这一次还没开始的（包括交给别人的，它开始时，前面的事项要赶的截止可能变），另有 `next_start`：其中最早的那个时刻（`2026-11-14` 或 `2026-11-14 09:30`），到时列表、顺序或 `due_by` 可能变，可以再取一次；它不受 `folders` 和 `limit` 限制 |
| `agenda` | `{overdue: [事项], days: [{date, tasks: [事项]}], marks}`：`overdue` 是截止日期在今天以前、还没完成的，按截止时间；`days` 只有有事项的日子，按日期；每天里有具体时间的按时间排在前面；重复事项以后的一次另有 `later: true`，它的 `due` 仍是现在这一次的；`marks` 是这几天里有节日、节气或放假、补班的日子，`{date, festival?, term?, holiday?}`，按日期，`holiday` 是 `off`（放假）或 `work`（补班），不受 `folders` 限制 |
| `show` | 事项加上 `created_at, updated_at`；`description`、`created_by`、`updated_by`（用户名，自己时没有）、`created_via`、`updated_via`（令牌名，没用令牌时没有）、`before: [事项]`、`after: [事项]` 有内容时才有 |
| `search` | `{tasks: [事项], total}` |
| `graph` | `{tasks: [事项], edges: [{before, after}]}`，边用编号，别人共享给我的文件夹里的事项写成 `"@alice#42"` |
| `log` | `[{batch, user, actor, source, created_at, summary, changes, undoes, undone}]`，`user` 是做的人的用户名，自己为 `null`；`actor` 是令牌名，没用令牌为 `null`；`source` 是文字里写的来源，如“本人（iPhone）”；`changes` 是每类变化一行，列出的事项和 `summary` 一样多；`undoes` 是撤销的批次号，只有整批就是撤销一个批次时才有（应用里的撤销都是这样），否则、或那个批次已不在记录里的为 `null`；`undone` 是已经撤销了（同[改动记录](design.md#改动记录和撤销)：撤销和撤销的撤销连成一串，和最新的一次方向相反的）。给了 `batch` 时另有 `items: [{text, change, fields: [{field, name, before, after}]}]`：改的每样东西一项，按改动的先后，`text` 是它的那一行（如“修改 #8 整理报销单”，事项的标题写全），`change` 是 `created`、`updated` 或 `deleted`：添加的事项、文件夹、依赖、附件、AI 整理请求和成员是 `created`，删除、移除的是 `deleted`；`name` 是字段的中文名（如“截止”），`field` 是 `title`、`description`、`folder`、`number`、`start`、`due`、`reminders`、`repeat`、`assignees`、`completions`、`order`、`name`、`parent`、`color`、`icon`、`holder` 之一，`before`、`after` 是文字里的写法（如“2026-11-20 18:00”“提前 1 小时、当天 09:00”“每周一”），颜色和图标是代码，没有值、或者是添加（删除）的另一边为 `null`，`order` 两边都是 `null` |
| `jobs` | `[{id, kind, start, folder, state, updated_at}]`，`kind` 是 `organize` 或 `tidy`，`start` 是原文开头，`state` 是 `waiting`、`running`、`failed`、`ready`，失败的另有 `error` |
| 写操作 | `{batch, changes: [变化]}`，预览时 `batch` 为 `null`；完成事项后有事项因此可做时另有 `unlocked: [事项]` |
| `attach`（命令行） | 每个文件一项写操作的数据，按给出的顺序组成一个数组；中途失败时只列已经加上的，再报错，退出码为 1 |
| `download`（命令行） | `{id, name, path}`：附件 ID、附件的名称、实际存到的路径 |

写操作的变化只说做了什么，不带事项的内容。一处改动同时做了几件事（比如撤销时一项既重新打开又改回了标题，一个文件夹既改名又移动），每件各列一条，文件夹都用这处改动之前的路径：

| `kind` | 其他字段 |
|---|---|
| `folder_created`、`folder_sorted`、`folder_deleted` | `folder`：路径 |
| `folder_renamed` | `folder`：原路径；`name`：新名字 |
| `folder_styled` | `folder`：路径；`color`、`icon`：改成的值，改回默认的是 `null`，没改的没有 |
| `folder_moved` | `folder`：原路径；`to`：新路径 |
| `task_created`、`task_done`、`task_dropped`、`task_reopened`、`task_deleted` | `task`：编号 |
| `task_moved` | `task`；`to`：文件夹路径；换了编号时另有 `number`：新编号 |
| `task_repeated` | `task`；`due`：重复事项完成或跳过后顺延到的截止日期；按完成日算的提前完成时，可能和原来同一天或更早 |
| `task_edited` | `task`；`fields`：改了的字段，`title`、`description`、`start`（开始日期或时刻）、`due`、`completions`（重复事项完成的次数，撤销一次完成时会变）、`assignees`、`order`（在同一文件夹里的顺序） |
| `attachment_added`、`attachment_deleted` | `attachment`：附件 ID；`name`：名称；`task`：所属事项的编号，属于 AI 整理请求的是 `null` |
| `attachment_renamed` | `attachment`；`name`：新名称 |
| `attachment_moved` | `attachment`；`task`：移到的事项编号，移回 AI 整理请求的是 `null` |
| `member_added`、`member_removed` | `folder`：路径；`user`：用户名 |
| `person_renamed` | `user`：用户名；`name`：新的显示名 |
| `dep_added`、`dep_removed` | `before`、`after`：编号 |
| `job_created`、`job_started`、`job_retried`、`job_ready`、`job_deleted` | `job`：请求 ID |
| `job_failed` | `job`；`error`：原因 |

需要改动前后完整内容的程序用 [`/changes`](api.md#端点)。

## 命令行

安装：

- **桌面 App**：在设置的“命令行”一组点“安装”，App 更新时命令行一起更新，见[桌面 App](desktop.md#命令行)。
- **macOS 和 Linux**：运行安装脚本（`apps/site/public/install.sh`，随官网发布）。它下载最新版到 `~/.local/bin`，这个目录不在 `PATH` 里时，给出要加进 shell 配置文件的一行：

  ```sh
  curl -fsSL https://juniexd.github.io/NextTodo/install.sh | sh
  ```

- **手动下载**：从 [Releases](https://github.com/JunieXD/NextTodo/releases/latest) 下载对应平台的文件，改名为 `nexttodo`（Windows 是 `nexttodo.exe`），放进 `PATH` 里的目录。macOS 会拦下用浏览器下载的文件，下载后运行 `xattr -d com.apple.quarantine <文件>`。

  | 平台 | 文件 |
  |---|---|
  | macOS（Apple 芯片） | `nexttodo-macos-arm64` |
  | macOS（Intel） | `nexttodo-macos-x64` |
  | Linux x64 | `nexttodo-linux-x64` |
  | Linux arm64 | `nexttodo-linux-arm64` |
  | Windows x64 | `nexttodo-windows-x64.exe` |

- **从源码**：装了 Rust 的，运行 `cargo install --git https://github.com/JunieXD/NextTodo nexttodo-cli`。

装好后登录，再把 skill 装给 agent：

```sh
nexttodo login --url https://todo.example.com --name "Claude Code"
nexttodo skill install                   # 见 Skill
```

- 命令行和服务端用同一个版本：服务端升级后，桌面 App 里装的随 App 更新，其他方式装的重新装一次；`nexttodo --version` 显示版本。
- 也可以不登录，用 App 里生成的令牌设置下面的环境变量，令牌页面会给出填好的写法。

- `login` 用用户名和密码登录，生成一个以 `--name` 命名（默认“命令行”）的读写令牌，加 `--read-only` 生成只读令牌。令牌存在 `~/.config/nexttodo/config.json`（权限 600），登录用的会话随即注销。
- 环境变量 `NEXTTODO_URL`、`NEXTTODO_TOKEN` 优先于配置文件，可以给不同的 agent 用不同的令牌，改动记录里就能分清是谁做的。
- `NEXTTODO_URL` 写 `local` 时，连这台电脑上[本机模式](local.md#agent-接入)的 App。
- 网络出错时，写操作用同一个改动 ID 自动重试两次，不会重复生效；`--request-id` 指定改动 ID，见[重试](#重试)。
- `nexttodo export [<文件>]` 把这个账号的数据下载成备份文件，默认存成当前目录下的 `NextTodo-<用户名>-<日期>.tar`，不覆盖已有的文件（先写 `.partial`，下完再改名；`.partial` 已经有了就停下，可能是另一个导出正在写）；`nexttodo import <备份文件>` 把本机模式的备份（或导出的文件）加进这个账号，输出加了多少和批次号，用 `nexttodo undo <批次>` 撤销；没收到回复时提示加上 `--request-id` 重发，不会导入两次。连本机模式时这两个命令提示用 App 的备份和恢复。见[搬到服务器和搬回本机](local.md#搬到服务器和搬回本机)。

## MCP

- 地址 `https://todo.example.com/mcp`，请求头 `Authorization: Bearer <令牌>`。
- [本机模式](local.md#agent-接入)下地址每次启动都变，改用命令行的 `nexttodo mcp` 转接（stdio）：它把每条消息转给 `NEXTTODO_URL` 指向的服务端。
- 连接时服务端给出一段使用说明（MCP 的 `instructions`），是 [Skill](#skill) 的要点：只接 MCP、不装 skill，agent 也知道该怎么用。
- 工具的结果是上面的文本；出错时返回 `isError` 和错误说明。`attachment` 读到图片时，结果里在文本后面另有一个图片内容块。
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
- `POST /api/v1/tools/{name}`：请求体是参数 JSON（最大 28 MB，够 `attach` 带上 20 MB 文件的 base64），返回 `{text, data}`；`attachment` 的 `data` 里，图片的内容是 base64 的 `image`，文本文件的是 `text`；读工具还返回 `seq`：结果包含到这个序号为止的全部改动，编号同 [`/changes`](api.md#端点)。错误格式同 [API](api.md#错误)。
- 写操作的改动 ID 用请求头 `Idempotency-Key: <uuidv7>` 或参数 `request_id` 给，见[重试](#重试)。

## 重试

每个写操作都有一个改动 ID。用同一个 ID 重发，生效过的不会再生效一次，原样返回第一次的结果（90 天内；超过保留期，或者这个 ID 是 App 提交的改动 ID 时，只返回批次号）。

| 入口 | 改动 ID |
|---|---|
| 命令行 | 每条写命令自动生成，网络出错时用它自动重试两次；都没有回应，或者网关（反向代理）代为回答 502、503、504 时，错误里给出它；`--request-id <uuidv7>` 指定 |
| MCP | 写工具的参数 `request_id`（UUIDv7），不给时服务端生成 |
| 工具接口 | 请求头 `Idempotency-Key` 或参数 `request_id`，都给时必须相同，否则 400 |

[本机模式](local.md#排定先后)借网盘同步时，写操作最多等 15 秒，还没写进网盘的返回 503：

- `unavailable`：还没发出，没有生效，可以直接重发。
- `outcome_unknown`：已经发出，网盘没有回应，可能已经生效。`details.mutation_id` 是这次的改动 ID，MCP 的错误说明末尾写作“request_id：<ID>”，命令行的写作“--request-id <ID>”。用它原样重发：已经生效的返回第一次的结果，还在等网盘的再等一次。

## Skill

`skills/nexttodo/SKILL.md` 教 agent 什么时候用、怎么用 NextTodo。命令行带着同一版本的一份，用它安装：

```sh
nexttodo skill install                   # 装到本机已有的 Claude Code、Codex：~/.claude/skills、~/.codex/skills
nexttodo skill install --dir <目录>       # 装到其他 agent 的 skill 目录，可以给多个
```

- 写到 `<目录>/nexttodo/SKILL.md`，覆盖已有的；升级命令行后再运行一次。
- `<目录>/nexttodo` 是链接时跳过，比如开发时把它链接到了仓库里的 `skills/nexttodo`。
