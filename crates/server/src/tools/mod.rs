//! Tools for agents and scripts: one implementation behind the command line, MCP and
//! `/api/v1/tools`. See `docs/agent.md`.

pub(crate) mod args;
mod read;
mod text;
pub(crate) mod write;

pub use write::plan_into;
pub(crate) use write::{Edit, edit_fields, new_due};

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use nexttodo_core::board::{Board, Now};
use nexttodo_core::model::{Date, FolderId, MutationId, Person, Task, Time};
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::JsonObject;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::AppState;
use crate::auth::Auth;
use crate::drive::{self, Route};
use crate::error::{ApiError, Json, from_value};
use crate::files::Files;
use crate::store;
use args::*;
use write::{Write, WriteCx};

/// The largest arguments a tool takes: `attach`'s, a 20 MB file in base64 (docs/agent.md#工具接口).
pub const MAX_BODY: usize = 28 * 1024 * 1024;

/// What a tool returns: text for people and agents, data for programs.
#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub text: String,
    pub data: Value,
}

/// A tool's answer as sent. A read tool's also says what it is as of: it includes every change up
/// to `seq`, as `/changes` numbers them.
#[derive(Serialize)]
pub struct Reply {
    #[serde(flatten)]
    pub output: Output,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Read,
    Write,
    /// Writes that remove things.
    Destructive,
}

#[derive(Serialize)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(skip)]
    pub kind: Kind,
    pub input_schema: Arc<JsonObject>,
}

/// Context for read tools.
pub struct Cx<'a> {
    pub user: &'a str,
    pub now: i64,
    /// The user's day and time, or UTC's if they don't say.
    pub local: Now,
    pub files: &'a Files,
}

type ReadFn = Box<dyn FnOnce(&Connection, &Cx) -> Result<Output, ApiError> + Send>;
/// Builds a write tool's operations; again each time the sync task tries it, from the data then.
pub(crate) type WriteFn = Box<dyn Fn(&WriteCx) -> Result<Write, ApiError> + Send + Sync>;

enum Call {
    Read(ReadFn),
    Write(WriteFn),
}

macro_rules! tools {
    ($($kind:ident $name:literal($args:ty) => $run:path, $desc:literal;)*) => {
        pub fn list() -> Vec<Tool> {
            vec![$(Tool { name: $name, description: $desc, kind: Kind::$kind, input_schema: with_common(schema_for_type::<$args>(), Kind::$kind) }),*]
        }

        fn parse_call(name: &str, args: Value) -> Result<Call, ApiError> {
            match name {
                $($name => {
                    let args: $args = from_value(args)?;
                    Ok(tools!(@call $kind, $run, args))
                })*
                _ => Err(ApiError::not_found(format!("不存在名为 {name} 的工具"))),
            }
        }
    };
    (@call Read, $run:path, $args:ident) => { Call::Read(Box::new(move |c, cx| $run(c, cx, $args))) };
    (@call $kind:ident, $run:path, $args:ident) => { Call::Write(Box::new(move |w| $run(w, $args.clone()))) };
}

tools! {
    Read "tree"(NoArgs) => read::tree, "文件夹树，每个文件夹里（含子文件夹）的待办数和可做数；共享的文件夹注明共享人和成员，他人共享给我的列在后面。";
    Read "list"(ListArgs) => read::list, "列出一个文件夹的子文件夹和事项，事项按先后顺序；默认不含已完成的，最多 50 项。文件夹里有 AI 整理请求时开头注明；在共享的文件夹里时开头写共享人和成员。";
    Read "next"(NextArgs) => read::next, "接下来：当前可做的事项（前置均已完成），有截止时间的排在前面，最多 50 项；共享的文件夹里只列交给我的和没指定负责人的。回答“现在该做什么”时使用。";
    Read "agenda"(AgendaArgs) => read::agenda, "日程：今天起几天（默认 7 天，最多 31 天）每天截止的事项，重复事项以后的每一次也列在那天并注明；前面另列已逾期的。共享的文件夹里只列交给我的和没指定负责人的。回答“这周有什么要做”“哪天有什么截止”时使用。";
    Read "show"(ShowArgs) => read::show, "事项详情：状态、文件夹、负责人、截止时间和提醒、前置、后续、附件、来源和描述。";
    Read "search"(SearchArgs) => read::search, "按关键词搜索事项的标题和描述，不区分大小写，最多列出 50 项。";
    Read "graph"(GraphArgs) => read::graph, "文件夹（含子文件夹）里的事项和依赖，按先后顺序列出，每项后面是它的前置；连到文件夹外的事项也列出。";
    Read "log"(LogArgs) => read::log, "最近的改动批次，新的在前，包括其他成员在共享的文件夹里做的。撤销时要用批次号。";
    Read "jobs"(NoArgs) => read::jobs, "用户提交的 AI 整理请求：等待、正在整理和失败的，失败的带原因。整理好的已经变成事项，不在这里。";
    Read "attachment"(AttachmentArgs) => read::attachment, "读一个附件（ID 见 show）：PNG、JPEG、WebP、GIF 图片（5 MB 以内）返回图片，UTF-8 文本文件（1 MB 以内）返回内容，其他文件只给名称、类型和大小。";
    Write "add"(AddArgs) => write::add, "添加一个事项；文件夹不存在时一起新建。一次加多个、带依赖的事项时用 apply。";
    Write "edit"(EditArgs) => write::edit, "修改事项的标题、描述、截止时间、提醒或负责人；只改给出的字段，due、remind 给 null 表示清除，assign、unassign 在现有的负责人上加、减。改描述中的一段用 replace、加内容用 append，不要读出整段再整段写回，以免盖掉用户同时做的修改。";
    Write "done"(DoneArgs) => write::done, "把事项标为完成，结果里会列出因此变成可做的事项。重复的事项不会完成，而是把截止时间顺延到下一次，这时要给 today；打卡的长期事项全部做完时用 end_repeat 结束重复并完成。";
    Write "drop"(DropArgs) => write::drop_tasks, "放弃事项：不做了，和完成一样结束、不再挡着后续，结果里会列出因此变成可做的事项；重复的事项整个结束，只是这一次不做用 skip。with_after 连同还没结束的后续一起放弃。";
    Write "reopen"(TasksArgs) => write::reopen, "重新打开已完成或放弃的事项。";
    Write "skip"(TasksArgs) => write::skip, "跳过重复事项的这一次：截止时间顺延到下一次，但不算完成，要给 today。已是最后一次的不能跳过，不再做了用 done 的 end_repeat。";
    Write "mkdir"(MkdirArgs) => write::mkdir, "新建文件夹，路径上缺的层级一起建。";
    Write "rename"(RenameArgs) => write::rename, "给文件夹或附件改名。";
    Write "move"(MoveArgs) => write::move_to, "把事项或文件夹移到一个已有的文件夹，/ 是根目录。移进、移出共享的文件夹时去掉连到外面的依赖和无法查看新位置的负责人，并换成新主人的编号。";
    Write "style"(StyleArgs) => write::style, "设文件夹的颜色和图标，只用在文件夹的图标上；给 null 改回默认，不给的不改。";
    Write "copy"(CopyArgs) => write::copy, "复制文件夹，连同其中的子文件夹、事项、附件和它们之间的依赖，副本里的事项都是待办，开始、截止、提醒和重复不变。用来把一套流程当模板反复用。";
    Destructive "delete"(DeleteArgs) => write::delete, "删除事项或文件夹；文件夹连同其中的子文件夹和事项一并删除。可用 undo 撤销。";
    Write "dep_add"(DepArgs) => write::dep_add, "加一条依赖：before 完成后才能做 after。只能连私人的事项之间，或同一个共享的文件夹里的事项之间；会成环时拒绝，并列出环上的事项。";
    Write "dep_remove"(DepArgs) => write::dep_remove, "删除一条依赖。";
    Write "attach"(AttachArgs) => write::attach, "给事项加一个附件，文件内容用 base64 给出，最多 20 MB，原样保存。";
    Destructive "detach"(DetachArgs) => write::detach, "删除附件。可用 undo 撤销。";
    Write "share"(ShareArgs) => write::share, "把自己的文件夹共享给同一台服务器上的其他人，或添加成员：成员能查看和修改里面的一切。";
    Destructive "unshare"(UnshareArgs) => write::unshare, "共享人移除成员，不写用户就是停止共享；成员不写用户就是自己退出共享。";
    Write "apply"(ApplyArgs) => write::apply, "按计划批量新建文件夹、事项和依赖，全部成功或全部不生效。新事项用 ref 互相引用，已有事项用编号。先用 dry_run 预览给用户看。";
    Write "undo"(UndoArgs) => write::undo, "撤销一个批次（批次号见写操作的结果或 log）。撤销本身也是一个新批次。看得到全部改动、也有权做的批次才能撤销。";
}

/// A tool's arguments and `today` and `time`, which every tool takes (docs/agent.md#引用); a write's
/// also `request_id` (docs/agent.md#重试).
fn with_common(schema: Arc<JsonObject>, kind: Kind) -> Arc<JsonObject> {
    let mut s = (*schema).clone();
    let properties = s.entry("properties").or_insert_with(|| json!({}));
    properties["today"] = json!({
        "type": "string",
        "description": "用户当地今天的日期，如 2026-11-08：开始时间到了没有按它算，完成重复的事项时必须给，下一次的截止时间据此计算。不给时按 UTC 的现在",
    });
    properties["time"] = json!({
        "type": "string",
        "description": "用户当地现在的时间，如 18:30，和 today 一起给：开始时间到了没有按它算。只给 today 时，那天开始的都算已经到了",
    });
    if kind != Kind::Read {
        properties["request_id"] = json!({
            "type": "string",
            "description": "这次改动的 ID，UUIDv7，可以不给。返回“可能已经生效”时，用错误里给的 request_id 原样重发：生效过的只返回第一次的结果，不会再生效一次",
        });
    }
    Arc::new(s)
}

#[derive(Deserialize)]
struct Local {
    today: Option<Date>,
    time: Option<Time>,
}

/// The `today` and `time` given with a tool's arguments, taken out of them.
fn take_local(args: &mut Value) -> Result<Option<Now>, ApiError> {
    let Some(o) = args.as_object_mut() else { return Ok(None) };
    let given = from_value::<Local>(json!({ "today": o.remove("today"), "time": o.remove("time") }))?;
    match given {
        Local { today: Some(today), time } => Ok(Some(Now { today, time })),
        Local { today: None, time: None } => Ok(None),
        Local { today: None, time: Some(_) } => Err(ApiError::invalid("给了 time 也要给 today", Some("today".into()))),
    }
}

#[derive(Deserialize)]
struct RequestId {
    request_id: Option<MutationId>,
}

/// A write's change id: `request_id`, as given with its arguments, or `header`, the `Idempotency-Key`
/// sent with them.
fn change_id(request_id: Option<Value>, header: Option<MutationId>) -> Result<Option<MutationId>, ApiError> {
    let RequestId { request_id } = from_value(json!({ "request_id": request_id }))?;
    match (header, request_id) {
        (Some(header), Some(id)) if header != id => {
            Err(ApiError::invalid("Idempotency-Key 和 request_id 不一致", Some("request_id".into())))
        }
        (header, id) => Ok(header.or(id)),
    }
}

/// The folder at a path; `None` is the root.
fn find_folder(b: &Board, path: &str) -> Result<Option<FolderId>, ApiError> {
    b.find_folder(path).ok_or_else(|| ApiError::not_found(format!("未找到文件夹 /{}", path.trim().trim_matches('/'))))
}

fn find_task<'a>(b: &Board<'a>, r: &TaskRef) -> Result<&'a Task, ApiError> {
    let owner = match &r.user {
        Some(name) => person(b, name)?.id.as_str(),
        None => b.me(),
    };
    b.task_by_number(owner, r.number).ok_or_else(|| ApiError::not_found(format!("不存在事项 {r}")))
}

/// Someone in the user's shared folders by username.
fn person<'a>(b: &Board<'a>, username: &str) -> Result<&'a Person, ApiError> {
    b.person_named(username.trim()).ok_or_else(|| ApiError::not_found(format!("没有共享文件夹的成员“{}”", username.trim())))
}

/// Runs a tool. `id`, the `Idempotency-Key` sent with it, makes a write idempotent, as `request_id` in
/// `args` does: the same id returns the first result.
pub async fn call(st: &AppState, auth: Auth, name: &str, mut args: Value, id: Option<MutationId>) -> Result<Reply, ApiError> {
    let now = st.clock.now();
    let local = take_local(&mut args)?;
    // Read tools make no change, and leave it be.
    let request_id = args.as_object_mut().and_then(|o| o.remove("request_id"));
    let files = st.files.clone();
    match parse_call(name, args)? {
        // The read and the sequence number come from the same call on the one connection: no
        // change can come between them.
        Call::Read(run) => {
            st.db
                .call(move |c| {
                    let local = local.unwrap_or_else(|| Now::utc(now));
                    let output = run(c, &Cx { user: &auth.user_id, now, local, files: &files })?;
                    Ok(Reply { output, seq: Some(store::latest_seq(c, &auth.user_id)?) })
                })
                .await
        }
        Call::Write(build) => {
            auth.require_write()?;
            let id = change_id(request_id, id)?.unwrap_or_else(|| MutationId(uuid::Uuid::now_v7()));
            let _direct = match st.drive.route().await {
                Route::Drive(d) => {
                    let output = d.tool(auth.into(), id, build, local, Some(Instant::now() + drive::WAIT)).await?;
                    return Ok(Reply { output, seq: None });
                }
                Route::Direct(held) => held,
            };
            let hub = st.hub.clone();
            st.db
                .call(move |c| {
                    let (output, to) = write::run(c, &auth, &files, id, now, local, &build)?;
                    hub.notify(&to);
                    Ok(Reply { output, seq: None })
                })
                .await
        }
    }
}

pub async fn list_tools() -> Json<Vec<Tool>> {
    Json(list())
}

pub async fn call_tool(
    State(st): State<AppState>,
    auth: Auth,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(args): Json<Value>,
) -> Result<Json<Reply>, ApiError> {
    let id = headers
        .get("idempotency-key")
        .map(|v| {
            v.to_str()
                .ok()
                .and_then(|s| s.parse::<uuid::Uuid>().ok())
                .and_then(|id| MutationId::try_from(id).ok())
                .ok_or_else(|| ApiError::invalid("Idempotency-Key 必须是 UUIDv7", Some("Idempotency-Key".into())))
        })
        .transpose()?;
    call(&st, auth, &name, args, id).await.map(Json)
}
