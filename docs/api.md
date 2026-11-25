# API

服务端对外的接口。表结构和校验规则见[数据模型](data-model.md)。

## 认证

- 请求头 `Authorization: Bearer <令牌>`；网页用 HttpOnly Cookie `nt_session`，两者任选其一。
- 令牌明文是 `nt_` 加 43 位 base62 随机串（256 位熵），只在生成时返回一次，数据库只存它的 SHA-256。
- 只读令牌调用会产生改动的接口时返回 403。
- Cookie 属性：`HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=34560000`（400 天）。页面经 HTTP 打开时（设置和清除 Cookie 的请求，`Origin` 以 `http://` 开头）不带 `Secure`，否则浏览器不存，见[网页端](web.md#经-http-打开)。
- 带请求体的接口只接受 `Content-Type: application/json`。别的网站用表单提交不了这种请求，所以不能借用户的 Cookie 调接口。上传附件的 `PUT /files/{id}` 例外，请求体是文件本身：表单发不了 PUT，别的网站同样借不了 Cookie。
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
| POST | `/auth/verify` | 核对密码：`{password}`，对了返回 204，不对返回 400；只能用会话。[隐私锁](design.md#隐私锁)用账号密码解锁时用 |
| GET | `/tokens` | 当前用户的会话和 API 令牌（不含明文）。令牌、邀请码和升级只能用会话管理，API 令牌调用返回 403 |
| POST | `/tokens` | 生成 API 令牌：`{name, scope}` → `{token, secret}` |
| DELETE | `/tokens/{id}` | 注销会话或删除令牌 |
| GET | `/invites` | 管理员：自己生成的邀请码 |
| POST | `/invites` | 管理员：生成一个邀请码 |
| GET | `/admin/upgrade` | 管理员：[在设置里升级](deploy.md#在设置里升级)的状态 `{state, …}`。`state` 是 `idle`；`downloading`，带 `version`、`received`、`total`（字节数）；`switching`，带 `version`；`failed`，带 `version` 和给用户看的 `message`；不能这样升级的（开发版）是 `unsupported` |
| POST | `/admin/upgrade` | 管理员：开始升级到 `{version}`，返回状态；正在下载或切换时只返回当前的状态。版本号要比服务端的新，否则 400 |
| GET | `/snapshot` | 完整数据：`{user, seq, folders, tasks, dependencies, completions, attachments, ai_jobs, shares, people}`，`user` 是数据所属用户的 ID。包括共享给这个用户的文件夹和其中的一切，见[共享](#共享) |
| GET | `/changes?since=<seq>` | 增量：`{seq, changes}`；`since` 早于保留期时返回 410 |
| POST | `/mutations` | 提交一个改动，见下文 |
| GET | `/sync` | WebSocket，见[同步协议](#同步协议) |
| PUT | `/files/{id}` | 上传附件的文件，见[附件](#附件) |
| GET | `/files/{id}` | 下载附件的文件 |
| GET | `/export` | 这个用户自己的数据，打成[备份](local.md#备份)的 tar，`?today=YYYY-MM-DD` 是文件名里的日期；见[导出](local.md#导出)。本机模式没有 |
| POST | `/import` | 把上传了的备份加进这个账号：`{id, database, files}` → `{tasks, folders, batch}`，加了几个事项、几个文件夹，和批次序号。`id` 是客户端生成的改动 ID，重发不会导入两次，撤销也用它。见[导入](local.md#导入)；只读令牌返回 403，本机模式没有 |
| GET | `/settings` | 服务器的设置：`{compress_images}`，是否在上传前[压缩图片](design.md#附件) |
| PUT | `/settings` | 管理员改服务器的设置，本机模式下是本机用户：`{compress_images}` → 设置；只能用会话 |
| GET | `/tools` | 工具列表，见 [agent 接入](agent.md#工具接口) |
| POST | `/tools/{name}` | 调用一个工具 |
| | `/ai/…` | AI 整理的服务商、配置和测试，见 [AI 整理](ai.md#接口) |
| GET | `/local/sync` | 本机模式：[借网盘同步](local.md#借网盘同步)的情况 `{drive, status, stopped}`。开着时 `drive` 是网盘（不含密码和密钥），`status` 是 `{enabling, synced_at, problem}`：还在开启、上次网盘回应的时间、受阻的原因；`stopped` 是同步自己停下的原因。这三个接口只能用会话 |
| PUT | `/local/sync` | 本机模式：开启，`{config, replace}`。`config` 是 `{kind: "webdav", url, user, password, folder}` 或 `{kind: "s3", endpoint, region, bucket, access_key, secret_key, path_style, folder}`。文件夹里已有数据时，`replace` 为 false 返回 409 `has_data`，为 true 时准备好网盘里的数据，下次启动换上，App 随即重启。成功返回 204 |
| DELETE | `/local/sync` | 本机模式：停止同步，先下载在用的附件里本机还没有的文件。下载不了时返回 409 `files_not_here`，带 `?force=true` 不下载、直接停止。成功返回 204 |

MCP 端点是 `/mcp`（不在 `/api/v1` 下），见 [agent 接入](agent.md#mcp)。

`seq` 是这个用户能看到的最新改动序号；`/snapshot` 和 `/changes` 返回的数据都截止到这个序号。

## 共享

一个用户看得到的是自己的数据，加上别人[共享](design.md#共享)给自己的文件夹和其中的一切。共享的文件夹里的东西属于共享人，事项编号也是共享人的。

- `/snapshot` 的 `shares` 是这个用户共享出去的和共享给这个用户的文件夹，`people` 是这个用户自己、这些文件夹里的人，和这个用户看得到的事项的添加人、最后修改人（可能已经离开）。
- 别人共享的文件夹在这个用户的数据里没有上级：`parent_id` 为空，共享人把它放在哪里都一样，也不会暴露共享人别的文件夹。
- 加入、退出或被移出共享的人，设备收到 `resync`，重新下载完整数据。

## 附件

[附件](design.md#附件)分两步加：先上传文件，再用改动 `attachment.add` 把它加为附件。网页端在文件上传完以前不发这个改动，见[网页端](web.md#附件)。

- `PUT /files/{id}`：请求体是文件，`Content-Type` 是它的类型（`类型/子类型`，最长 100 字；不知道的用 `application/octet-stream`）。`id` 是客户端生成的 UUIDv7，之后也是附件的 ID。最多 20 MB，超过时返回 413。服务端边收边写进 `files/` 下的临时文件，收完以后在数据库的锁里再查一次这个 ID：没人用过就改名换上、记下这个文件，返回 `{id, size}`；同一个人已经传过（断线重传）就丢掉这份，直接返回原来的；别人用过返回 409。已经记下的文件不会被覆盖。只读令牌返回 403。
- `PUT /files/{id}?from=<附件 ID>`：不带请求体，把看得到的附件 `from` 的文件复制一份，作为 `id` 的文件记下，之后和上传的一样；`from` 看不到时返回 404。[复制文件夹](design.md#文件夹)时用，文件不经过客户端。
- `GET /files/{id}`：文件。看得到它现在所属的事项或请求的人能下载；它不属于任何事项或请求时（还没加为附件，或者附件删除后文件还留着），上传的人也能。其余返回 404。响应头 `Content-Type` 是附件的类型，并且总有 `Content-Disposition: attachment; filename*=UTF-8''<名称>`、`X-Content-Type-Options: nosniff` 和 `Content-Security-Policy: sandbox`：上传的 HTML、SVG 即使在浏览器里被直接打开，也不会在这个网址下执行脚本、借用登录状态。谁能读、叫什么名字会变，所以 `Cache-Control: no-store`，每次都问服务端；App 自己存着下载过的文件，见[网页端](web.md#附件)。
- 跨域：这两个接口和 `GET /export`（[导出](local.md#导出)）允许来源 `https://localhost`（Android App 的页面）和 `capacitor://localhost`（iOS App 的页面），方法 GET、PUT，请求头只允许 `Authorization` 和 `Content-Type`，不带 Cookie，页面读得到响应头 `Content-Disposition`（导出的文件名）；预检请求不要求登录。手机 App 的其他请求走原生 HTTP，附件的文件和导出的备份大，经原生和网页之间的桥传会出问题，见 [Android](android.md#附件)。
- 日志只记附件 ID、大小和结果，不记文件名。

## 实体

```jsonc
// 文件夹
{ "id": "…", "parent_id": null, "name": "毕业论文", "sort_key": "a0", "color": "blue", "icon": "graduation-cap", "created_at": 1730000000000, "updated_at": 1730000000000 }
// color、icon 没选时为 null，取值见 data-model.md 的 folders
// 事项
{ "id": "…", "number": 5, "folder_id": "…", "title": "设计问卷", "description": "…", "done_at": null, "dropped": false, "start": "2026-10-30", "start_time": "09:00",
  // done_at 是结束的时间，待办为 null；dropped 是[放弃](design.md#放弃)了，这时 done_at 是放弃的时间
  // 开始时间的日期和几点几分：没设时 start 为 null，只有日期时没有 start_time，见 design.md#开始时间
  "due": { "date": "2026-11-02", "time": "18:00", "reminders": [{ "days": 1, "time": "20:00" }, { "before": 60 }], "repeat": { "every": 1, "unit": "week", "from": "due" } },
  // 没有截止时间时为 null；time、reminders、repeat 可省略；reminders 是提醒：{"before": 分钟} 提前多久（0 是截止时，要有 time，最多 43200），{"days": 天数, "time": "20:00"} 截止那天（0）或提前几天（最多 30）的几点，按到点的先后排、不重复，最多 100 个；旧版本写的 remind_before（提前多少分钟）读入时换成一个提前多久的提醒，不再写出；按周的 repeat 可以带 "days": [1, 3, 5]，在周几（1 是周一，7 是周日）；按月的 days 是几号（1–31）。每年是 every 12、unit month。repeat 可以带 "until": "2026-12-19"，重复到哪天（含这天）。按农历的按月 repeat 带 "lunar": true，这时一定有 days，是农历的几号（1–30，初一到三十），见[重复](design.md#重复)
  "due_after": { "days": 30, "time": "18:00", "reminders": [{ "before": 60 }] }, "unlocked_on": "2026-11-24",
  // 前置完成后几天内截止的才有，见下面的[前置完成后的截止](#前置完成后的截止)：due_after 同 due 去掉 date 和 repeat，days 是 0–365；
  // unlocked_on 是前置都完成的那天，没完成时没有这一项，这时 due 为 null
  "completions": { "count": 12, "last": "2026-11-01" },
  // 重复的事项完成过几次、最近一次的日期；没完成过时没有这一项。只随完成改变，改动里不能直接写
  "sort_key": "a1", "assignees": ["<用户 id>"],
  // 负责人，只有共享的文件夹里的事项能有
  "created_at": …, "updated_at": …, "created_by": "<用户 id>", "updated_by": "<用户 id>", "created_via": "Claude Code", "updated_via": null }
// 附件：task_id 和 job_id 正好一个有值
{ "id": "…", "task_id": "…", "job_id": null, "name": "图片 1.png", "type": "image/png", "size": 182044, "created_at": … }
// 依赖
{ "before": "<前置 id>", "after": "<后续 id>", "created_at": … }
// AI 整理请求
{ "id": "…", "folder_id": "…", "text": "…", "local_time": "2026-11-04 16:30 星期三", "state": "failed", "error": "API 密钥无效（接口返回 401：…）",
  "created_at": …, "updated_at": … }
// 共享的文件夹：共享人和成员的用户 id
{ "folder_id": "…", "owner": "<用户 id>", "members": ["<用户 id>"] }
// 人：自己，和共享的文件夹里的其他人
{ "id": "…", "username": "bob", "display_name": "Bob" }
// 用户
{ "id": "…", "username": "alice", "display_name": "Alice", "is_admin": true }
// 邀请码
{ "code": "Kx7Pq2Lm9AbC", "created_at": …, "used_by": "friend", "used_at": … }  // used_by 是注册者的用户名
// 令牌（不含明文）
{ "id": "…", "kind": "api", "name": "Claude Code", "scope": "write", "created_at": …, "last_used_at": …, "current": false }
```

## 改动

`POST /mutations` 的请求体，也是同步协议里 `mutate` 消息的内容。一个改动包含一个或多个操作，在一个事务里全部生效或全部不生效。可选的 `today`、`time` 是发出改动的设备上的日期和时间，见[前置完成后的截止](#前置完成后的截止)；给了 `time` 就要给 `today`。

```json
{ "id": "<uuidv7>", "today": "2026-11-24", "time": "23:30", "ops": [
  { "op": "task.create", "id": "<uuidv7>", "folder_id": "<id>", "title": "写初稿" },
  { "op": "dep.add", "before": "<id>", "after": "<上面新建的 id>" }
] }
```

| 操作 | 字段 | 说明 |
|---|---|---|
| `folder.create` | `id`、`name`，可选 `parent_id`、`sort_key`、`color`、`icon` | 不给 `sort_key` 时排在同一层最后 |
| `folder.update` | `id`，可选 `name`、`parent_id`、`sort_key`、`color`、`icon` | `parent_id: null` 表示移到根目录；`color`、`icon` 给 `null` 表示改回默认 |
| `folder.delete` | `id` | 连同子文件夹、事项和相关依赖 |
| `task.create` | `id`、`title`，可选 `folder_id`、`description`、`start`、`start_time`、`due` 或 `due_after`、`sort_key`、`done`、`assignees` | `assignees` 是负责人的用户 ID |
| `task.update` | `id`，可选 `folder_id`、`title`、`description`、`start`、`start_time`、`due` 或 `due_after`、`sort_key`、`done`、`today`、`occurrence`、`assign`、`unassign` | 只带要改的字段；`start: null` 清除开始时间（连同几点）；只给 `start` 时几点不变，`start_time: null` 只清除几点；有 `start_time` 就要有开始日期；`due: null` 清除截止时间（连同重复和 `due_after`），给了日期就不再是前置完成后的；`due_after: null` 清除它和它给的截止日期；`done: true/false` 完成或重新打开，放弃了的完成时改成完成、结束的时间不变；`assign`、`unassign` 是要加上、去掉的负责人，两个人同时指派时都算数 |
| `task.drop` | `id` | [放弃](design.md#放弃)：结束并标为放弃，重复的事项去掉重复，不记完成；已完成的改成放弃、结束的时间不变，已经放弃的不变。连同后续一起放弃是一个改动里的几个 `task.drop` |
| `task.delete` | `id` | 连同相关依赖 |
| `dep.add` | `before`、`after` | |
| `dep.remove` | `before`、`after` | |
| `attachment.add` | `id`、`name`、`type`、`size`，`task_id` 或 `job_id` | 把上传了的文件加为事项或 AI 整理请求的附件，规则见[数据模型](data-model.md#规则) |
| `attachment.update` | `id`，可选 `name`、`task_id` | 改名，或改属另一个事项（AI 整理完成时、按原文添加时从请求改到事项） |
| `attachment.delete` | `id` | |
| `ai_job.create`、`ai_job.retry`、`ai_job.delete` | | 提交、重试、删除 AI 整理请求，见 [AI 整理](ai.md#接口) |
| `share.add` | `folder_id`、`username` | 共享人按用户名添加成员 |
| `share.remove` | `folder_id`、`user_id` | 共享人移除成员，或成员自己退出。最后一个成员离开时文件夹回到私人的 |
| `undo` | `mutation` | 撤销一个改动：按相反顺序把它的变化恢复回去，恢复时同样要通过所有规则。要改回去的东西已经不存在的跳过；要恢复的事项或文件夹原来所在的文件夹、要恢复的依赖连着的事项已经被删除时，整个撤销不生效 |

`task.update` 把有[重复](design.md#重复)的事项标为完成（`done: true`）时不记完成时间，而是按 `today`（发出改动的设备上的当天，`YYYY-MM-DD`）把截止日期顺延到下一次，`completions` 的次数加一、日期记为 `today`；这时缺了 `today` 返回 400。顺延出的下一次晚于重复的 `until` 时，这次就是最后一次：次数同样加一、日期记为 `today`，然后去掉重复、记完成时间。同一个改动里的 `due` 先生效，所以去掉重复的同时 `done: true` 就是结束重复并完成：记完成时间，不动 `completions`，也不看 `today`。别的事项只用 `today` 定下[前置完成后的截止](#前置完成后的截止)。带上 `occurrence`（要完成的那一次：`{due, completions}`，它的截止日期和那时完成过的次数）时，重复的事项现在已经不是这一次的，这次完成不生效，改动照样成功：那一次已经在别处完成、跳过或改了日期，再完成就会把下一次也完成掉。只看日期不够：从完成那天算的提前完成了，下一次可能还是同一天截止。App 和网页上勾选完成（包括批量完成）、手机的通知、提醒卡片和小组件上的“完成”都给重复的事项带着它：列表可能过时，几台设备也可能离线时各自完成了同一次；结束重复并完成不带；不重复的事项不看 `occurrence`。[放弃](design.md#放弃)了的事项不再有下一次，带 `occurrence` 的完成对它不生效，不带的把它改成完成。已结束（完成或放弃）的事项不能有重复，截止日期不能晚于重复的 `until`，新建或修改出这样的事项返回 400。

### 前置完成后的截止

事项可以用 `due_after` 代替 `due`：前置都完成后几天内截止（[设计](design.md#前置完成后的截止)）。`due` 由服务端算出，改动里不能同时给两个，同时给返回 400（`field` 是 `due_after`）：

- 每个改动的操作都生效以后，服务端看这个改动涉及的有 `due_after` 的事项：新建或改了 `due_after`、`start` 的，前置完成、重新打开或删除的，连上或去掉前置的。前置都不挡着它（同[状态](design.md#状态)，重复的前置这一次没开始也算）、又还没定下的，`unlocked_on` 记为这个改动的“今天”；定下了、而这个改动让某个前置变成挡着它的（重新打开、连上没完成的，包括撤销时恢复的连线），去掉 `unlocked_on`；做过的重复前置（有 `completions`）不算，它挡着只是又到了它的一次。别的时候不动，重复的前置到了下一次不去掉。
- 定下以后 `due` 是 `unlocked_on` 和 `start` 里晚的那天加 `days`，几点和提醒照 `due_after`；没定下时 `due` 为 `null`。这些变化是同一个改动的效果，一起撤销。
- “今天”依次看：改动的 `today`；改动里 `task.update` 带的 `today`（网页和 App 上勾选完成都带着它，离线时完成、联网后补发的也按完成那天算）；都没有时按 UTC。网页和 App 只在这台设备上有 `due_after` 的事项、或这个改动设了它时才带 `today`、`time`，免得不认它们的旧服务端拒收所有改动。
- `days` 是 0 的，开始时间和 `due_after` 都有几点时，开始的几点不能晚于截止的，否则返回 400。不能设重复：要设重复就给 `due`。

成功返回 `{seq, effects}`。同一个 `id` 再次提交时直接返回第一次的结果，不会重复生效；超过保留期的只返回 `seq`，`effects` 为空。

## 变化

`effects` 是一个改动实际产生的实体变化，按发生顺序排列：

```json
[ { "kind": "task", "before": null, "after": { …新事项… } },
  { "kind": "dependency", "before": null, "after": { "before": "…", "after": "…", "created_at": … } } ]
```

`kind` 是 `folder`、`task`、`dependency`、`completion`、`attachment`、`ai_job`、`share` 或 `person`。`before` 为空表示新建，`after` 为空表示删除，都不为空表示修改。`completion` 是重复事项的一条[完成记录](design.md#重复) `{task_id, due, done, again}`，`again` 是同一次（同一个 `due`）在这一回以前完成过几回，通常是 0；和 `dependency` 一样只有新建和删除，由完成重复的事项、删除事项和撤销带来，改动里不能直接写。客户端按顺序应用 `after` 就能得到最新状态。`share` 是一个文件夹的共享人和成员，有了成员是新建，最后一个成员离开是删除；`person` 是这个用户的数据里新出现的人（新建：加入了共享的文件夹，或者添加、修改过一个刚看得到的事项）或改了显示名的人（修改）。

每个人收到的是自己看得到的那部分：事项移出共享的文件夹，留下的成员收到的是它被删除，移进来时是新建，它的附件、依赖也一样。`/changes` 和同步推送里的每条记录是 `{seq, id, user, actor, effects, created_at}`，`id` 是改动 ID，`user` 是提交改动的用户 ID，`actor` 是 API 令牌名（会话为空，AI 整理是“AI 整理”）。

## 错误

非 2xx 响应的格式是 `{code, message, details?}`，`message` 是给用户看的中文说明。

| 状态 | code | 情况 |
|---|---|---|
| 400 | `invalid` | 字段格式、长度不对，`details.field` 指出哪一项；改动里的操作只定位到第几个，如 `ops[2]` |
| 401 | `unauthenticated` | 没有令牌、令牌无效，或用户名密码不对 |
| 403 | `forbidden` | 只读令牌提交改动或上传文件，非管理员管理邀请码、升级服务端、改服务器的设置，成员改名、移动、删除共享的文件夹或管理成员，或撤销看不全的改动 |
| 404 | `not_found` | 引用的实体不存在或当前用户看不到 |
| 409 | `cycle` | 依赖会成环，`details.path` 是环上的事项 ID，首尾相同 |
| 409 | `folder_cycle` | 文件夹移到了自己的子孙下面 |
| 404 | `file_missing` | 加附件时服务器上没有这个文件：还没上传，或者上传后一直没加为附件、已被清理。客户端重新上传后再发 |
| 409 | `conflict` | 用户名已被占用、邀请码已用过、依赖已存在、同一层已有同名文件夹、附件 ID 已被别人用过 |
| 409 | `has_data` | 开启借网盘同步时，网盘的这个文件夹里已有数据 |
| 409 | `files_not_here` | 停止借网盘同步时，在用的附件有文件还没能从网盘下载下来 |
| 413 | `too_large` | 上传的文件超过 20 MB |
| 410 | `resync` | `since` 早于保留期，或者服务端恢复过快照，需要重新下载 `/snapshot` |
| 503 | `unavailable` | 本机模式借网盘同步时连不上网盘：改动还没发出，没有生效，见[排定先后](local.md#排定先后)；开启时连不上网盘 |
| 503 | `outcome_unknown` | 本机模式借网盘同步时，改动已经发出，网盘没有及时回应，可能已经生效；`details.mutation_id` 是改动 ID，用它原样重发不会重复生效，见[重试](agent.md#重试)；保存 AI 整理的说明没有改动 ID，再保存一次即可 |

## 同步协议

`GET /sync` 升级为 WebSocket，消息都是 JSON。消息是小的增量，不压缩；完整数据走 `/snapshot`。客户端的用法见[网页端](web.md#同步)。

| 方向 | 消息 | 说明 |
|---|---|---|
| 客户端 → 服务端 | `{type: "hello", token?, since, user}` | 连接后 10 秒内必须发的第一条。网页靠 Cookie 认证可以不带 `token`；App 的 WebView 不能给 WebSocket 加请求头，所以带在这里。`user` 是本地副本所属的用户 ID：登录的已经是别人（比如在另一个标签页换了账号）时，连接以 `unauthenticated` 失败，这份副本不会收到别人的数据，也不会把改动提交到别人名下 |
| 服务端 → 客户端 | `{type: "changes", seq, changes}` | 收到 `hello` 后先补发 `since` 之后的变化（没有也发一条空的，表示已经同步到最新），之后每有新改动就推送，包括本连接自己提交的。`seq` 是这批之后的最新序号，每条变化的格式同 `/changes` |
| 服务端 → 客户端 | `{type: "resync"}` | `since` 超出保留期或服务端恢复过快照，客户端重新下载 `/snapshot` |
| 客户端 → 服务端 | `{type: "mutate", mutation}` | 提交改动，格式同 `POST /mutations` |
| 服务端 → 客户端 | `{type: "ack", id, seq}` | 改动已生效，`seq` 是它的序号（重复提交时是第一次生效时的序号） |
| 服务端 → 客户端 | `{type: "reject", id, code, message, details?}` | 改动本身不成立（格式不对、冲突、事项不存在、只读令牌等），客户端撤回它的本地效果，不再重发。`file_missing` 例外：附件的文件不在服务器上，客户端重新上传后再发同一个改动，见[网页端](web.md#附件) |
| 服务端 → 客户端 | `{type: "error", code, message}` | 认证失败、消息无法识别或服务端出错（`internal`），随后服务端关闭连接。提交改动时会话已失效或服务端出错，也是这样而不是 `reject`：错不在改动，客户端留着它，重新登录或重连后再发 |

- 每次 `mutate` 和每次推送前都重新检查令牌：令牌删除、会话退出或改密码登出其他设备后，已有的连接马上收到 `unauthenticated` 错误并关闭，不再收到数据；只读令牌的提交被拒绝。
- Cookie 只在页面与服务端同源时有效（请求的 `Origin` 与 `Host` 或 `X-Forwarded-Host` 一致，不区分大小写），以免同一站点下的其他页面借用 Cookie 建立连接。浏览器总会带 `Origin`；没有 `Origin` 的连接不认 Cookie，App 用 `hello` 里的 `token`。
- 服务端每 30 秒发一次 ping，反向代理不会因为空闲断开连接；75 秒内没有收到客户端的任何消息（包括 pong）就关闭连接。
