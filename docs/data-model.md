# 数据模型

服务端的 SQLite 数据库。概念和规则的产品层定义见[总体设计](design.md#概念和规则)。

## 约定

- SQLite，WAL 模式，`STRICT` 表，开启外键。
- 时间戳是 UTC 的 Unix 毫秒整数。
- 截止日期和时间是浮动的本地时间：日期 `YYYY-MM-DD`，时间 `HH:MM`，不带时区。“18:00 截止”在哪个时区看都是当地 18:00。
- 实体 ID 是 UUIDv7 文本。文件夹、事项和改动的 ID 由客户端生成，离线时就能互相引用；用户和令牌的 ID 由服务端生成。
- 顺序键是分数索引字符串（base62，与 `fractional-indexing` 库的算法一致），按字节序比较。插到两项之间时生成介于两者之间的新键，不改动其他行。

## 表

### users

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | |
| username | TEXT UNIQUE（不区分大小写） | 登录名，3–32 位字母、数字、下划线 |
| display_name | TEXT | 显示名 |
| password_hash | TEXT | Argon2id，PHC 字符串 |
| is_admin | INTEGER | 0 或 1 |
| next_task_number | INTEGER | 下一个事项编号，从 1 开始 |
| pruned_seq | INTEGER | 改动日志已删除到的序号，客户端的 `since` 比它小就要重新下载；加入、退出或被移出[共享](design/sharing.md)时设为那个改动的序号，让这个人的设备重新下载 |
| created_at | INTEGER | |

### invites

| 列 | 类型 | 说明 |
|---|---|---|
| code | TEXT PK | 随机邀请码 |
| created_by | TEXT → users | |
| used_by | TEXT → users，可空 | 用过后填上 |
| created_at / used_at | INTEGER | |

### tokens

会话和 API 令牌放在同一张表。

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | |
| user_id | TEXT → users | |
| kind | TEXT | `session`（登录设备）或 `api`（给 agent 和命令行） |
| name | TEXT | 设备名或令牌名，用作改动来源的显示名 |
| scope | TEXT | `read` 或 `write`；会话总是 `write` |
| secret_hash | BLOB UNIQUE | 令牌明文的 SHA-256 |
| created_at / last_used_at | INTEGER | |

### folders

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | |
| owner_id | TEXT → users | 共享的文件夹和里面的一切都属于共享人 |
| parent_id | TEXT → folders，可空 | 空表示在根目录；删除父文件夹时级联删除 |
| name | TEXT | 1–100 字，单行，不含 `/` 和控制字符 |
| sort_key | TEXT | 同一层内的顺序 |
| color | TEXT，可空 | [颜色](ui.md#文件夹的颜色和图标)：预设的 `red`、`orange`、`amber`、`yellow`、`lime`、`green`、`teal`、`cyan`、`blue`、`indigo`、`purple`、`pink`、`brown`、`gray`，或自己调的 `#rrggbb`（写的时候大小写都行，存成小写）；空表示默认 |
| icon | TEXT，可空 | [图标](ui.md#文件夹的颜色和图标)：`briefcase`、`graduation-cap`、`book`、`pen`、`file`、`laptop`、`code`、`flask`、`calculator`、`lightbulb`、`target`、`house`、`cart`、`utensils`、`coffee`、`shirt`、`baby`、`paw`、`heart`、`gift`、`dumbbell`、`bike`、`heart-pulse`、`pill`、`wallet`、`piggy-bank`、`plane`、`car`、`map-pin`、`music`、`camera`、`palette`、`gamepad`、`star`、`flag`、`rocket`、`trophy`、`wrench`、`leaf`、`users`，依次是界面上那 40 个图标；空表示默认 |
| created_at / updated_at | INTEGER | |

同一个用户同一层的文件夹不重名：唯一索引 `(owner_id, coalesce(parent_id, ''), name)`。

### tasks

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | |
| owner_id | TEXT → users | |
| number | INTEGER | 每个用户内唯一的短编号；换了主人（移进、移出别人共享的文件夹）就换成新主人的 |
| folder_id | TEXT → folders，可空 | 空表示在根目录；删除文件夹时级联删除 |
| title | TEXT | 1–200 字，单行，不含控制字符 |
| description | TEXT | Markdown，最长 20000 字；除换行和制表符外不含控制字符 |
| done_at | INTEGER，可空 | 空表示待办，有值是结束的时间 |
| dropped | INTEGER | [放弃](design.md#放弃)了为 1，否则为 0；为 1 时 `done_at` 有值，是放弃的时间 |
| start_date | TEXT，可空 | [开始时间](design/dates.md#开始时间)的日期，没到时事项等着；有 `due_date` 时开始不能晚于截止 |
| start_time | TEXT，可空 | 开始的几点几分，只能在有 `start_date` 时设置；没有时从那天 0 点开始 |
| start_reminders | TEXT，可空 | 按开始时间算的[提醒](design/dates.md#提醒)，JSON 列表，写法同 `reminders`；只能在有 `start_date` 时设置，提前多久的要有 `start_time`；没有时为空 |
| remind_at | TEXT，可空 | 指定时间的提醒，JSON 列表 `[{date, time}]`，按时刻先后排、不重复；没有时为空 |
| due_date | TEXT，可空 | |
| due_time | TEXT，可空 | 只能在有 `due_date` 时设置 |
| reminders | TEXT，可空 | 按截止时间算的[提醒](design/dates.md#提醒)，JSON 列表，写法同 [API](api.md) 的 `reminders`；提前多久的只能在有 `due_time` 时设置；没有提醒时为空 |
| repeat_every | INTEGER，可空 | [重复](design/repeat.md#重复)的间隔数，1–365；只能在有 `due_date` 时设置 |
| repeat_unit | TEXT，可空 | `day`、`week` 或 `month` |
| repeat_from | TEXT，可空 | 下一次从哪天算：`due`（截止日期）或 `done`（完成那天） |
| repeat_days | INTEGER，可空 | 按周重复在周几，按位存：周一是 1，周二是 2，……，周日是 64；只能在 `repeat_unit` 为 `week` 时设置，空表示截止日期那天 |
| repeat_monthdays | INTEGER，可空 | 按月重复在几号，按位存：1 号是 1，2 号是 2，3 号是 4，……，31 号是 2³⁰；只能在 `repeat_unit` 为 `month` 时设置，空表示截止日期那天 |
| repeat_lunar | INTEGER | 按[农历](design/repeat.md#重复)重复为 1，否则为 0；为 1 时 `repeat_unit` 是 `month`，`repeat_monthdays` 不为空，只到 30 号（初一到三十） |
| repeat_legal | INTEGER | [每个法定工作日](design/repeat.md#重复)为 1，否则为 0；为 1 时 `repeat_every` 是 1，`repeat_unit` 是 `week`，`repeat_days` 是周一到周五 |
| repeat_until | TEXT，可空 | [重复到](design/repeat.md#重复)哪天（含这天），空表示一直重复；只能在有重复时设置，不早于 `due_date` |
| due_after | TEXT，可空 | [前置完成后几天内](design/dates.md#前置完成后的截止)截止，JSON，写法同 [API](api.md#前置完成后的截止) 的 `due_after`；有它时 `due_date` 等几列是它给的截止时间，前置没都完成时为空 |
| unlocked_on | TEXT，可空 | 前置都完成的那天，截止日期从它（或更晚的 `start_date`）算起；只能在有 `due_after` 时设置 |
| completion_count | INTEGER，可空 | 重复的事项[完成过几次](design/repeat.md#重复)，至少 1；从没完成过时为空。结束重复并完成不算 |
| last_completed | TEXT，可空 | 最近一次完成的日期（完成那台设备上的当天） |
| sort_key | TEXT | 文件夹内的手动顺序 |
| created_at / updated_at | INTEGER | |
| created_by / updated_by | TEXT | 添加和最后修改它的用户 ID，共享的文件夹里可能不是主人 |
| created_via / updated_via | TEXT，可空 | 用 API 令牌改动时记令牌名，AI 整理添加的记“AI 整理”；本人在设备上改动时为空 |

`(owner_id, number)` 唯一。重复的三列同时有值或同时为空，已完成的事项（`done_at` 有值）不能有。开始、截止、提醒和重复的约束同时用 `CHECK` 写在表上（三种提醒加起来最多 100 个），`completion_count` 和 `last_completed` 也一样同时有值或同时为空。

### completions

重复事项的[完成记录](design/repeat.md#重复)，每次完成一条。

| 列 | 类型 | 说明 |
|---|---|---|
| task_id | TEXT | 事项，删除事项时一起删除 |
| due_date | TEXT | 完成的是截止在哪天的那一次 |
| again | INTEGER | 这一次在这一回以前完成过几回：第一回是 0，同一次又完成一回是 1，依此类推 |
| done_on | TEXT | 在哪天完成（完成那台设备上的当天） |

`(task_id, due_date, again)` 是主键：同一次完成了几回就有几条。

### dependencies

| 列 | 类型 | 说明 |
|---|---|---|
| before_id | TEXT → tasks | 前置 |
| after_id | TEXT → tasks | 后续 |
| owner_id | TEXT → users | |
| created_at | INTEGER | |

主键 `(before_id, after_id)`，`CHECK (before_id <> after_id)`；任一端的事项被删除时级联删除。

### members

[共享](design/sharing.md)的文件夹的成员，共享人不在里面。一个文件夹有成员就是共享的，最后一个成员离开时回到私人的。

| 列 | 类型 | 说明 |
|---|---|---|
| folder_id | TEXT → folders | 共享的文件夹；删除时级联删除 |
| user_id | TEXT → users | 成员 |

主键 `(folder_id, user_id)`，另有 `user_id` 上的索引，用来找一个人看得到的文件夹。

### assignees

事项的负责人，只有共享的文件夹里的事项有，都是这个文件夹的共享人或成员。

| 列 | 类型 | 说明 |
|---|---|---|
| task_id | TEXT → tasks | 删除事项时级联删除 |
| user_id | TEXT → users | |

主键 `(task_id, user_id)`。

### attachments

事项和 AI 整理请求的[附件](design/attachments.md)。

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | 客户端生成，也是文件的 ID（`files.id`） |
| task_id | TEXT → tasks，可空 | 所属的事项；删除事项时级联删除 |
| job_id | TEXT → ai_jobs，可空 | 所属的 AI 整理请求；删除请求时级联删除。和 `task_id` 正好一个有值 |
| name | TEXT | 1–200 字，单行，不含控制字符、`/` 和 `\`，不是 `.` 或 `..` |
| created_at | INTEGER | |

`task_id`、`job_id` 上各有索引。看得到所属的事项或请求的人，看得到它。附件的类型和大小只存在文件那一行，接口里的附件连同文件的这两项一起给出。

### files

附件的文件，存在数据目录的 `files/<id>` 里，不经同步协议传给设备；借网盘同步时传进网盘，各设备按需下载，见[附件](local/drive.md#附件)。

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | |
| owner_id | TEXT → users | 上传的人，只有他能把它加为附件 |
| type | TEXT | MIME 类型，如 `image/png` |
| size | INTEGER | 字节数，1 到 20 MB |
| created_at | INTEGER | |
| unused_at | INTEGER，可空 | 没有附件用它时，从何时起：上传后还没加为附件，或附件已删除；有附件用它时为空 |
| purged | INTEGER | 0 或 1：借网盘同步时已清理，文件删了、行留着，这个 ID 不能再用 |
| staged | INTEGER | 0 或 1：借网盘同步时只上传了、还没在网盘的提交里加为附件，是这台设备自己的 |

已清理的行一定没人用（`unused_at` 不为空），也不再是哪台设备自己的（`staged` 为 0），由表的约束保证。

每天清理一次：在一个事务里删掉 `unused_at` 在 90 天以前的行，再删它们的文件；行删了以后，加附件和撤销都用不上它了，所以不会删掉刚恢复的文件。`files/` 里没有对应行的文件（写到一半的，或写完、还没记下就中断的）一天以后也删掉：在数据库的锁里逐个再查一次没有行才删，上传的文件也在这把锁里换上、记下，所以不会删掉刚记下的。借网盘同步时改为写进网盘、各设备一起清理，见[附件](local/drive.md#附件)。

### server_settings

只有一行，服务器的设置。

| 列 | 类型 | 说明 |
|---|---|---|
| id | INTEGER PK | 固定为 1 |
| compress_images | INTEGER | 上传前是否[压缩图片](design/attachments.md)，默认 1 |

### ai_jobs

[AI 整理](ai.md)的请求。整理成功时请求删除，整理出的事项随之添加；[整理文件夹](ai.md#整理文件夹)完成时请求留下，带着建议，等用户应用或放弃。

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | 客户端生成 |
| owner_id | TEXT → users | |
| folder_id | TEXT → folders，可空 | 整理出的事项放进这里，或要整理的就是这里的事项；空表示根目录；删除文件夹时级联删除 |
| kind | TEXT | `organize`（整理请求）或 `tidy`（整理文件夹） |
| text | TEXT | 原文，或整理文件夹的要求；1–10000 字；除换行和制表符外不含控制字符 |
| local_time | TEXT | 提交时的本地时间，如 `2026-11-04 16:30 星期三`，单行，最长 40 字 |
| reminders | TEXT，可空 | 提交（或重试）整理的设备的[默认提醒](ai.md#提示词和结果)，JSON `{timed, dated}`：`timed` 是 `[{before}]`，`dated` 是 `[{days, time}]`，各自排好序、不重复，每样最多 100 个；那台设备没设的为空 |
| state | TEXT | `waiting`（等待）、`running`（整理中）、`failed`（失败）或 `ready`（整理文件夹的建议待确认） |
| error | TEXT，可空 | 失败的原因，只在 `failed` 时有 |
| suggestions | TEXT，可空 | 建议，JSON 数组，每条按独有的字段区分：修改 `{task, title?, description?, due?, done?, folder?, after, unlink?, reason, was}`（`due` 为 `null` 是清除）、删除 `{delete, reason, was}`、新建 `{create, title, description?, due?, after, before, reason}`（`create` 是新事项的 ID）、调整顺序 `{order, reason}`、改文件夹的颜色和图标 `{style, color?, icon?, reason, was}`（`style` 是文件夹 ID，`null` 是改回默认）；修改和删除的 `was` 是整理时事项的 `{title, description, due}`，改颜色和图标的是文件夹的 `{color, icon}`。只在 `ready` 时有 |
| run | TEXT，可空 | 借网盘同步时正在整理的这一轮：开始那次改动的 ID，只在 `running` 时有，见[运行](local/drive.md#运行) |
| created_at / updated_at | INTEGER | |

### ai_settings

同步到服务器的 AI 整理配置，每个用户最多一份。

| 列 | 类型 | 说明 |
|---|---|---|
| user_id | TEXT PK → users | |
| provider | TEXT | 预设服务商（`deepseek`、`qwen`、`openai`）或 `custom` |
| base_url | TEXT | 接口地址，`https://` 开头 |
| model | TEXT | |
| thinking | TEXT | `off`、`low` 或 `high` |
| vision | INTEGER | 模型能否识别图片，0 或 1，见 [AI 整理](ai.md#附件) |
| api_key | BLOB | 加密的 API 密钥：24 字节随机 nonce 加 XChaCha20-Poly1305 密文，附加数据是用户 ID，所以换到别人名下解不开 |
| updated_at | INTEGER | |

### ai_notes

AI 整理的[说明](ai.md#说明)，每个用户每个文件夹（含根目录）最多一份。

| 列 | 类型 | 说明 |
|---|---|---|
| user_id | TEXT → users | 写说明的人 |
| folder_id | TEXT，可空 | `NULL` 是根目录，即全局说明。不设外键：文件夹删除后说明留着，撤销删除时随文件夹回来 |
| text | TEXT | 1–2000 字 |
| updated_at | INTEGER | |

唯一索引 `(user_id, ifnull(folder_id, ''))`。

### mutations

改动日志，用于幂等、增量同步和撤销。

| 列 | 类型 | 说明 |
|---|---|---|
| seq | INTEGER PK AUTOINCREMENT | 全局递增序号 |
| id | TEXT UNIQUE | 客户端生成的改动 ID |
| owner_id | TEXT → users | 提交改动的用户 |
| token_id | TEXT | 提交改动的令牌；服务端自己做的改动（AI 整理）为空字符串 |
| actor | TEXT | 来源显示名的快照：会话为空，API 令牌为令牌名，AI 整理为“AI 整理” |
| ops | TEXT（JSON），可空 | 提交的操作 |
| effects | TEXT（JSON），可空 | 实际产生的全部实体变化，见 [API](api.md#变化) |
| reply | TEXT（JSON），可空 | 通过工具做的改动：工具的回复，同一个幂等键重发时原样返回 |
| created_at | INTEGER | |

操作、变化和回复保留 90 天：服务端每天把更早的行的 `ops`、`effects`、`reply` 清空，删掉对应的 `deliveries`，行本身留下，这样很久以前的改动被重新提交时仍然认得出来，不会再生效一次。按序号整段清理，所有用户的 `pruned_seq` 一起推进，只增不减。

### deliveries

每个改动发给谁、各人看到的是什么样子。只发给看得到其中变化的人，提交的人总在里面。

| 列 | 类型 | 说明 |
|---|---|---|
| user_id | TEXT → users | |
| seq | INTEGER → mutations | |
| effects | TEXT（JSON），可空 | 这个人看到的变化，和 `mutations.effects` 不同时才有：事项移出共享的文件夹，留下的成员看到的是它被删除；空表示看到全部 |

主键 `(user_id, seq)`。增量同步、改动记录和撤销都按这张表取一个人的改动。

[恢复快照](deploy.md#备份和恢复)时，换上的数据库接着此前的序号编号：`mutations` 的自增序号和每个用户的 `pruned_seq` 都设为快照和被替换的数据库里最大的序号加一，且不小于恢复时的毫秒时间戳，之前的行一并清空。设备记着的序号都比它小，于是收到 `resync`、重新下载；之后的改动序号也都比设备见过的大，小组件这样按序号取舍的列表照常更新。被替换的数据库坏了或丢了、读不出序号时，靠的是时间戳：此前的序号从 1 或从上次恢复时起逐个增加，只要时钟没有回拨到上次恢复之前，上次恢复以来的改动也没有快过一毫秒一个，就都比它小；否则见过更大序号的设备收不到 `resync`，留着恢复前的数据，要让那台设备联网同步之后退出登录、重新登录，才会重新下载（退出登录会删掉设备上还没发出的改动）。

### epoch

只有一行，记着这个数据库的代号：建库时随机生成，[恢复备份](local/backup.md#恢复)或[快照](deploy.md#备份和恢复)时换一个新的。序号只在同一个代号下有意义，见[数据代号](local.md#数据代号)。

| 列 | 类型 | 说明 |
|---|---|---|
| id | INTEGER PK | 固定为 1 |
| value | TEXT | 32 位十六进制的随机串 |

### sync_state

本机模式[借网盘同步](local/drive.md)时，这台设备在网盘历史里的位置；没开着时没有行。最多一行，和读入的数据在同一个事务里写。

| 列 | 类型 | 说明 |
|---|---|---|
| id | INTEGER PK | 固定为 1 |
| generation | TEXT | 同步的这一代的代号 |
| applied | INTEGER | 读到的提交的编号 |

### sync_runs

借网盘同步时，这台设备开始的 AI 整理，在写开始整理的提交之前记下，整理完或失败后删掉；启动时还在的记为失败，见[运行](local/drive.md#运行)。

| 列 | 类型 | 说明 |
|---|---|---|
| run | TEXT PK | 开始那次改动的 ID |
| job_id | TEXT | 整理的请求 |

## 规则

服务端应用改动时在事务里检查，任何一条不满足就整个改动不生效：

- 引用的文件夹、事项都必须存在，且当前用户看得到：自己的，或共享给自己的文件夹里的。共享的文件夹本身只有共享人能改名、改颜色和图标、移动、删除和管理成员。
- 文件夹不能移到自己或自己的子孙下面；同一层不能重名。
- 依赖不能指向自己、不能重复、不能成环；成环时返回环上的事项。两端在同一处：都是私人的，或在同一个共享的文件夹里。
- 负责人只能设在共享的文件夹里的事项上，须是这个文件夹的共享人或成员。共享的文件夹不能放进另一个共享的文件夹，里面也不能再共享文件夹。
- 事项或文件夹移进、移出共享的文件夹时，连到外面的依赖、看不到它的负责人一起去掉；换了主人的事项取新主人的编号。
- 截止时间和提醒满足上面的约束；标题、名称、描述满足长度限制。
- 删除文件夹时，子文件夹、事项、相关依赖、附件和其中的 AI 整理请求一起删除，全部记进同一个改动的变化里，撤销时一起恢复。删除事项、请求时它们的附件同样。删掉的附件，文件的 `unused_at` 记为当时，撤销恢复时清空。
- 加附件时：文件已上传，是本人上传的，还没加为附件，类型和大小与上传时一致；所属的事项看得到，请求是自己的。撤销删除时恢复附件只看所属的事项（或请求）在不在、看不看得到，以及文件还在不在（`files` 里有这一行），不看是谁上传的。
- 事项或文件夹移进、移出共享的文件夹时，事项的附件跟着走：变化里为没变的附件也记一笔，新看到的人收到附件新建，看不到了的人收到删除；撤销移动时同样。
- 新事项的编号取 `next_task_number` 并加一。撤销删除或撤销换主人的移动时恢复原编号，前提是这个号新主人发过（小于它的 `next_task_number`）且没被占用，否则取新号。
