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
| name | TEXT | 1–100 字，不含 `/` |
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
| title | TEXT | 1–200 字，单行 |
| description | TEXT | Markdown，最长 20000 字 |
| done_at | INTEGER，可空 | 空表示待办 |
| due_date | TEXT，可空 | |
| due_time | TEXT，可空 | 只能在有 `due_date` 时设置 |
| remind_before | INTEGER，可空 | 提前多少分钟提醒，0 表示截止时；只能在有 `due_time` 时设置 |
| sort_key | TEXT | 文件夹内的手动顺序 |
| created_at / updated_at | INTEGER | |
| created_via / updated_via | TEXT，可空 | 用 API 令牌改动时记令牌名；本人在设备上改动时为空 |

`(owner_id, number)` 唯一。截止和提醒的约束同时用 `CHECK` 写在表上。

### dependencies

| 列 | 类型 | 说明 |
|---|---|---|
| before_id | TEXT → tasks | 前置 |
| after_id | TEXT → tasks | 后续 |
| owner_id | TEXT → users | |
| created_at | INTEGER | |

主键 `(before_id, after_id)`，`CHECK (before_id <> after_id)`；任一端的事项被删除时级联删除。

### mutations

改动日志，用于幂等、增量同步和撤销。

| 列 | 类型 | 说明 |
|---|---|---|
| seq | INTEGER PK AUTOINCREMENT | 全局递增序号 |
| id | TEXT UNIQUE | 客户端生成的改动 ID |
| owner_id | TEXT → users | |
| token_id | TEXT | 提交改动的令牌 |
| actor | TEXT | 来源显示名的快照：会话为空，API 令牌为令牌名 |
| ops | TEXT（JSON），可空 | 提交的操作 |
| effects | TEXT（JSON），可空 | 实际产生的实体变化，见 [API](api.md#变化) |
| created_at | INTEGER | |

操作和变化保留 90 天：服务端每天把更早的行的 `ops`、`effects` 清空，行本身留下，这样很久以前的改动被重新提交时仍然认得出来，不会再生效一次。每个用户按序号整段清理，`pruned_seq` 只增不减。

## 规则

服务端应用改动时在事务里检查，任何一条不满足就整个改动不生效：

- 引用的文件夹、事项都必须存在且属于当前用户。
- 文件夹不能移到自己或自己的子孙下面；同一层不能重名。
- 依赖不能指向自己、不能重复、不能成环；成环时返回环上的事项。
- 截止时间和提醒满足上面的约束；标题、名称、描述满足长度限制。
- 删除文件夹时，子文件夹、事项和相关依赖一起删除，全部记进同一个改动的变化里，撤销时一起恢复。
- 新事项的编号取 `next_task_number` 并加一；撤销删除时恢复原编号。
