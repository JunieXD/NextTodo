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
| pruned_seq | INTEGER | 改动日志已删除到的序号，客户端的 `since` 比它小就要重新下载 |
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
| owner_id | TEXT → users | |
| parent_id | TEXT → folders，可空 | 空表示在根目录；删除父文件夹时级联删除 |
| name | TEXT | 1–100 字，单行，不含 `/` 和控制字符 |
| sort_key | TEXT | 同一层内的顺序 |
| created_at / updated_at | INTEGER | |

同一个用户同一层的文件夹不重名：唯一索引 `(owner_id, coalesce(parent_id, ''), name)`。

### tasks

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | |
| owner_id | TEXT → users | |
| number | INTEGER | 每个用户内唯一的短编号 |
| folder_id | TEXT → folders，可空 | 空表示在根目录；删除文件夹时级联删除 |
| title | TEXT | 1–200 字，单行，不含控制字符 |
| description | TEXT | Markdown，最长 20000 字；除换行和制表符外不含控制字符 |
| done_at | INTEGER，可空 | 空表示待办 |
| due_date | TEXT，可空 | |
| due_time | TEXT，可空 | 只能在有 `due_date` 时设置 |
| remind_before | INTEGER，可空 | 提前多少分钟提醒，0 表示截止时；只能在有 `due_time` 时设置 |
| sort_key | TEXT | 文件夹内的手动顺序 |
| created_at / updated_at | INTEGER | |
| created_via / updated_via | TEXT，可空 | 用 API 令牌改动时记令牌名，AI 整理添加的记“AI 整理”；本人在设备上改动时为空 |

`(owner_id, number)` 唯一。截止和提醒的约束同时用 `CHECK` 写在表上。

### dependencies

| 列 | 类型 | 说明 |
|---|---|---|
| before_id | TEXT → tasks | 前置 |
| after_id | TEXT → tasks | 后续 |
| owner_id | TEXT → users | |
| created_at | INTEGER | |

主键 `(before_id, after_id)`，`CHECK (before_id <> after_id)`；任一端的事项被删除时级联删除。

### ai_jobs

[AI 整理](ai.md)的请求。整理成功时请求删除，整理出的事项随之添加。

| 列 | 类型 | 说明 |
|---|---|---|
| id | TEXT PK | 客户端生成 |
| owner_id | TEXT → users | |
| folder_id | TEXT → folders，可空 | 整理出的事项放进这里；空表示根目录；删除文件夹时级联删除 |
| text | TEXT | 原文，1–10000 字；除换行和制表符外不含控制字符 |
| local_time | TEXT | 提交时的本地时间，如 `2026-11-04 16:30 星期三`，单行，最长 40 字 |
| state | TEXT | `waiting`（等待）、`running`（整理中）或 `failed`（失败） |
| error | TEXT，可空 | 失败的原因，只在 `failed` 时有 |
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
| api_key | BLOB | 加密的 API 密钥：24 字节随机 nonce 加 XChaCha20-Poly1305 密文，附加数据是用户 ID，所以换到别人名下解不开 |
| updated_at | INTEGER | |

### mutations

改动日志，用于幂等、增量同步和撤销。

| 列 | 类型 | 说明 |
|---|---|---|
| seq | INTEGER PK AUTOINCREMENT | 全局递增序号 |
| id | TEXT UNIQUE | 客户端生成的改动 ID |
| owner_id | TEXT → users | |
| token_id | TEXT | 提交改动的令牌；服务端自己做的改动（AI 整理）为空字符串 |
| actor | TEXT | 来源显示名的快照：会话为空，API 令牌为令牌名，AI 整理为“AI 整理” |
| ops | TEXT（JSON），可空 | 提交的操作 |
| effects | TEXT（JSON），可空 | 实际产生的实体变化，见 [API](api.md#变化) |
| reply | TEXT（JSON），可空 | 通过工具做的改动：工具的回复，同一个幂等键重发时原样返回 |
| created_at | INTEGER | |

操作、变化和回复保留 90 天：服务端每天把更早的行的 `ops`、`effects`、`reply` 清空，行本身留下，这样很久以前的改动被重新提交时仍然认得出来，不会再生效一次。每个用户按序号整段清理，`pruned_seq` 只增不减。

[恢复快照](deploy.md#备份和恢复)时，换上的数据库从恢复时的毫秒时间戳接着编号（快照里的序号更大时从它的下一个）：`mutations` 的自增序号和每个用户的 `pruned_seq` 都设为它，之前的行一并清空。此前的序号是从 1 或从上次恢复的时间戳起逐个增加的，改动不会一毫秒多于一个，所以都比它小，不用读被替换的数据库，那份坏了或丢了也一样。设备记着的序号都比它小，于是收到 `resync`、重新下载；之后的改动序号也都比设备见过的大，小组件这样按序号取舍的列表照常更新。

### epoch

只有一行，记着这个数据库的代号：建库时随机生成，[恢复备份](local.md#恢复)或[快照](deploy.md#备份和恢复)时换一个新的。序号只在同一个代号下有意义，见[数据代号](local.md#数据代号)。

| 列 | 类型 | 说明 |
|---|---|---|
| id | INTEGER PK | 固定为 1 |
| value | TEXT | 32 位十六进制的随机串 |

## 规则

服务端应用改动时在事务里检查，任何一条不满足就整个改动不生效：

- 引用的文件夹、事项都必须存在且属于当前用户。
- 文件夹不能移到自己或自己的子孙下面；同一层不能重名。
- 依赖不能指向自己、不能重复、不能成环；成环时返回环上的事项。
- 截止时间和提醒满足上面的约束；标题、名称、描述满足长度限制。
- 删除文件夹时，子文件夹、事项、相关依赖和其中的 AI 整理请求一起删除，全部记进同一个改动的变化里，撤销时一起恢复。
- 新事项的编号取 `next_task_number` 并加一；撤销删除时恢复原编号。
