# API

服务端对外的接口。表结构和校验规则见[数据模型](data-model.md)。

## 认证

- 请求头 `Authorization: Bearer <令牌>`；网页用 HttpOnly Cookie `nt_session`，两者任选其一。
- 令牌明文是 `nt_` 加 43 位 base62 随机串（256 位熵），只在生成时返回一次，数据库只存它的 SHA-256。
- 只读令牌调用会产生改动的接口时返回 403。
- Cookie 属性：`HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=34560000`（400 天）。
- 带请求体的接口只接受 `Content-Type: application/json`。别的网站用表单提交不了这种请求，所以不能借用户的 Cookie 调接口。
- 文件夹、事项和改动的 ID 必须是 UUIDv7。

## 端点

路径前缀 `/api/v1`，收发 JSON。

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/auth/register` | 凭邀请码注册：`{invite, username, password, display_name, device}`，成功后直接登录 |
| POST | `/auth/login` | `{username, password, device}` → `{secret, user}`，同时设置 Cookie |
| POST | `/auth/logout` | 注销当前会话，清除 Cookie；API 令牌调用返回 403 |
| GET | `/me` | 当前用户 |
| PATCH | `/me` | 改显示名：`{display_name}` → 用户 |
| POST | `/auth/password` | 改密码：`{old_password, new_password}`；只能用会话，成功后注销这个用户的其他会话 |
| GET | `/tokens` | 当前用户的会话和 API 令牌（不含明文）。令牌和邀请码只能用会话管理，API 令牌调用返回 403 |
| POST | `/tokens` | 生成 API 令牌：`{name, scope}` → `{token, secret}` |
| DELETE | `/tokens/{id}` | 注销会话或删除令牌 |
| GET | `/invites` | 管理员：自己生成的邀请码 |
| POST | `/invites` | 管理员：生成一个邀请码 |
| GET | `/snapshot` | 完整数据：`{user, seq, folders, tasks, dependencies, ai_jobs}`，`user` 是数据所属用户的 ID |
| GET | `/changes?since=<seq>` | 增量：`{seq, changes}`；`since` 早于保留期时返回 410 |
| POST | `/mutations` | 提交一个改动，见下文 |
| GET | `/sync` | WebSocket，见[同步协议](#同步协议) |
| GET | `/tools` | 工具列表，见 [agent 接入](agent.md#工具接口) |
| POST | `/tools/{name}` | 调用一个工具 |
| | `/ai/…` | AI 整理的服务商、配置和测试，见 [AI 整理](ai.md#接口) |

MCP 端点是 `/mcp`（不在 `/api/v1` 下），见 [agent 接入](agent.md#mcp)。

`seq` 是这个用户能看到的最新改动序号；`/snapshot` 和 `/changes` 返回的数据都截止到这个序号。

## 实体

```jsonc
// 文件夹
{ "id": "…", "parent_id": null, "name": "毕业论文", "sort_key": "a0", "created_at": 1730000000000, "updated_at": 1730000000000 }
// 事项
{ "id": "…", "number": 5, "folder_id": "…", "title": "设计问卷", "description": "…", "done_at": null,
  "due": { "date": "2026-11-02", "time": "18:00", "remind_before": 60, "repeat": { "every": 1, "unit": "week", "from": "due" } },
  // 没有截止时间时为 null；time、remind_before、repeat 可省略；按周的 repeat 可以带 "days": [1, 3, 5]，在周几（1 是周一，7 是周日）
  "sort_key": "a1", "created_at": …, "updated_at": …, "created_via": "Claude Code", "updated_via": null }
// 依赖
{ "before": "<前置 id>", "after": "<后续 id>", "created_at": … }
// AI 整理请求
{ "id": "…", "folder_id": "…", "text": "…", "local_time": "2026-11-04 16:30 星期三", "state": "failed", "error": "API 密钥无效（接口返回 401：…）",
  "created_at": …, "updated_at": … }
// 用户
{ "id": "…", "username": "alice", "display_name": "Alice", "is_admin": true }
// 邀请码
{ "code": "Kx7Pq2Lm9AbC", "created_at": …, "used_by": "friend", "used_at": … }  // used_by 是注册者的用户名
// 令牌（不含明文）
{ "id": "…", "kind": "api", "name": "Claude Code", "scope": "write", "created_at": …, "last_used_at": …, "current": false }
```

## 改动

`POST /mutations` 的请求体，也是同步协议里 `mutate` 消息的内容。一个改动包含一个或多个操作，在一个事务里全部生效或全部不生效。

```json
{ "id": "<uuidv7>", "ops": [
  { "op": "task.create", "id": "<uuidv7>", "folder_id": "<id>", "title": "写初稿" },
  { "op": "dep.add", "before": "<id>", "after": "<上面新建的 id>" }
] }
```

| 操作 | 字段 | 说明 |
|---|---|---|
| `folder.create` | `id`、`name`，可选 `parent_id`、`sort_key` | 不给 `sort_key` 时排在同一层最后 |
| `folder.update` | `id`，可选 `name`、`parent_id`、`sort_key` | `parent_id: null` 表示移到根目录 |
| `folder.delete` | `id` | 连同子文件夹、事项和相关依赖 |
| `task.create` | `id`、`title`，可选 `folder_id`、`description`、`due`、`sort_key`、`done` | |
| `task.update` | `id`，可选 `folder_id`、`title`、`description`、`due`、`sort_key`、`done`、`today` | 只带要改的字段；`due: null` 清除截止时间（连同重复）；`done: true/false` 完成或重新打开 |
| `task.delete` | `id` | 连同相关依赖 |
| `dep.add` | `before`、`after` | |
| `dep.remove` | `before`、`after` | |
| `ai_job.create`、`ai_job.retry`、`ai_job.delete` | | 提交、重试、删除 AI 整理请求，见 [AI 整理](ai.md#接口) |
| `undo` | `mutation` | 撤销一个改动：按相反顺序把它的变化恢复回去，恢复时同样要通过所有规则。要改回去的东西已经不存在的跳过；要恢复的事项或文件夹原来所在的文件夹、要恢复的依赖连着的事项已经被删除时，整个撤销不生效 |

`task.update` 把有[重复](design.md#重复)的事项标为完成（`done: true`）时不记完成时间，而是按 `today`（发出改动的设备上的当天，`YYYY-MM-DD`）把截止日期顺延到下一次；这时缺了 `today` 返回 400。别的事项不看 `today`。已完成的事项不能有重复，新建或修改出这样的事项返回 400。

成功返回 `{seq, effects}`。同一个 `id` 再次提交时直接返回第一次的结果，不会重复生效；超过保留期的只返回 `seq`，`effects` 为空。

## 变化

`effects` 是一个改动实际产生的实体变化，按发生顺序排列：

```json
[ { "kind": "task", "before": null, "after": { …新事项… } },
  { "kind": "dependency", "before": null, "after": { "before": "…", "after": "…", "created_at": … } } ]
```

`kind` 是 `folder`、`task`、`dependency` 或 `ai_job`。`before` 为空表示新建，`after` 为空表示删除，都不为空表示修改。客户端按顺序应用 `after` 就能得到最新状态。`/changes` 和同步推送里的每条记录是 `{seq, id, actor, effects, created_at}`，`id` 是改动 ID，`actor` 是 API 令牌名（会话为空，AI 整理是“AI 整理”）。

## 错误

非 2xx 响应的格式是 `{code, message, details?}`，`message` 是给用户看的中文说明。

| 状态 | code | 情况 |
|---|---|---|
| 400 | `invalid` | 字段格式、长度不对，`details.field` 指出哪一项；改动里的操作只定位到第几个，如 `ops[2]` |
| 401 | `unauthenticated` | 没有令牌、令牌无效，或用户名密码不对 |
| 403 | `forbidden` | 只读令牌提交改动，或非管理员管理邀请码 |
| 404 | `not_found` | 引用的实体不存在或不属于当前用户 |
| 409 | `cycle` | 依赖会成环，`details.path` 是环上的事项 ID，首尾相同 |
| 409 | `folder_cycle` | 文件夹移到了自己的子孙下面 |
| 409 | `conflict` | 用户名已被占用、邀请码已用过、依赖已存在、同一层已有同名文件夹 |
| 410 | `resync` | `since` 早于保留期，或者服务端恢复过快照，需要重新下载 `/snapshot` |

## 同步协议

`GET /sync` 升级为 WebSocket，消息都是 JSON。消息是小的增量，不压缩；完整数据走 `/snapshot`。客户端的用法见[网页端](web.md#同步)。

| 方向 | 消息 | 说明 |
|---|---|---|
| 客户端 → 服务端 | `{type: "hello", token?, since, user}` | 连接后 10 秒内必须发的第一条。网页靠 Cookie 认证可以不带 `token`；App 的 WebView 不能给 WebSocket 加请求头，所以带在这里。`user` 是本地副本所属的用户 ID：登录的已经是别人（比如在另一个标签页换了账号）时，连接以 `unauthenticated` 失败，这份副本不会收到别人的数据，也不会把改动提交到别人名下 |
| 服务端 → 客户端 | `{type: "changes", seq, changes}` | 收到 `hello` 后先补发 `since` 之后的变化（没有也发一条空的，表示已经同步到最新），之后每有新改动就推送，包括本连接自己提交的。`seq` 是这批之后的最新序号，每条变化的格式同 `/changes` |
| 服务端 → 客户端 | `{type: "resync"}` | `since` 超出保留期或服务端恢复过快照，客户端重新下载 `/snapshot` |
| 客户端 → 服务端 | `{type: "mutate", mutation}` | 提交改动，格式同 `POST /mutations` |
| 服务端 → 客户端 | `{type: "ack", id, seq}` | 改动已生效，`seq` 是它的序号（重复提交时是第一次生效时的序号） |
| 服务端 → 客户端 | `{type: "reject", id, code, message, details?}` | 改动本身不成立（格式不对、冲突、事项不存在、只读令牌等），客户端撤回它的本地效果，不再重发 |
| 服务端 → 客户端 | `{type: "error", code, message}` | 认证失败、消息无法识别或服务端出错（`internal`），随后服务端关闭连接。提交改动时会话已失效或服务端出错，也是这样而不是 `reject`：错不在改动，客户端留着它，重新登录或重连后再发 |

- 每次 `mutate` 和每次推送前都重新检查令牌：令牌删除、会话退出或改密码登出其他设备后，已有的连接马上收到 `unauthenticated` 错误并关闭，不再收到数据；只读令牌的提交被拒绝。
- Cookie 只在页面与服务端同源时有效（请求的 `Origin` 与 `Host` 或 `X-Forwarded-Host` 一致，不区分大小写），以免同一站点下的其他页面借用 Cookie 建立连接。浏览器总会带 `Origin`；没有 `Origin` 的连接不认 Cookie，App 用 `hello` 里的 `token`。
- 服务端每 30 秒发一次 ping，反向代理不会因为空闲断开连接；75 秒内没有收到客户端的任何消息（包括 pong）就关闭连接。
