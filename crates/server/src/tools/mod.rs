//! Tools for agents and scripts: one implementation behind the command line, MCP and
//! `/api/v1/tools`. See `docs/agent.md`.

pub(crate) mod args;
mod read;
mod text;
mod write;

pub use write::plan_into;
pub(crate) use write::{Edit, edit_fields, new_due, number};

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use nexttodo_core::board::Board;
use nexttodo_core::model::{FolderId, MutationId, Task};
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::JsonObject;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiError, Json, from_value};
use crate::store;
use args::*;
use write::{Write, WriteCx};

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
    pub owner: &'a str,
    pub now: i64,
}

type ReadFn = Box<dyn FnOnce(&Connection, &Cx) -> Result<Output, ApiError> + Send>;
type WriteFn = Box<dyn FnOnce(&WriteCx) -> Result<Write, ApiError> + Send>;

enum Call {
    Read(ReadFn),
    Write(WriteFn),
}

macro_rules! tools {
    ($($kind:ident $name:literal($args:ty) => $run:path, $desc:literal;)*) => {
        pub fn list() -> Vec<Tool> {
            vec![$(Tool { name: $name, description: $desc, kind: Kind::$kind, input_schema: schema_for_type::<$args>() }),*]
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
    (@call $kind:ident, $run:path, $args:ident) => { Call::Write(Box::new(move |w| $run(w, $args))) };
}

tools! {
    Read "tree"(NoArgs) => read::tree, "文件夹树，每个文件夹直接包含的待办数和可做数。";
    Read "list"(ListArgs) => read::list, "列出一个文件夹的子文件夹和事项，事项按先后顺序；默认不含已完成的，最多 50 项。文件夹里有 AI 整理请求时开头注明。";
    Read "next"(NextArgs) => read::next, "接下来：当前可做的事项（前置均已完成），有截止时间的排在前面，最多 50 项。回答“现在该做什么”时使用。";
    Read "show"(ShowArgs) => read::show, "事项详情：状态、文件夹、截止时间和提醒、前置、后续、来源和描述。";
    Read "search"(SearchArgs) => read::search, "按关键词搜索事项的标题和描述，不区分大小写，最多列出 50 项。";
    Read "graph"(GraphArgs) => read::graph, "文件夹（含子文件夹）里的事项和依赖，按先后顺序列出，每项后面是它的前置；连到文件夹外的事项也列出。";
    Read "log"(LogArgs) => read::log, "最近的改动批次，新的在前。撤销时要用批次号。";
    Read "jobs"(NoArgs) => read::jobs, "用户提交的 AI 整理请求：等待、正在整理和失败的，失败的带原因。整理好的已经变成事项，不在这里。";
    Write "add"(AddArgs) => write::add, "添加一个事项；文件夹不存在时一起新建。一次加多个、带依赖的事项时用 apply。";
    Write "edit"(EditArgs) => write::edit, "修改事项的标题、描述、截止时间或提醒；只改给出的字段，due、remind 给 null 表示清除。改描述中的一段用 replace、加内容用 append，不要读出整段再整段写回，以免盖掉用户同时做的修改。";
    Write "done"(DoneArgs) => write::done, "把事项标为完成，结果里会列出因此变成可做的事项。重复的事项不会完成，而是把截止时间顺延到下一次，这时要给 today；打卡的长期事项全部做完时用 end_repeat 结束重复并完成。";
    Write "reopen"(TasksArgs) => write::reopen, "重新打开已完成的事项。";
    Write "mkdir"(MkdirArgs) => write::mkdir, "新建文件夹，路径上缺的层级一起建。";
    Write "rename"(RenameArgs) => write::rename, "给文件夹改名。";
    Write "move"(MoveArgs) => write::move_to, "把事项或文件夹移到一个已有的文件夹，/ 是根目录。";
    Destructive "delete"(DeleteArgs) => write::delete, "删除事项或文件夹；文件夹连同其中的子文件夹和事项一并删除。可用 undo 撤销。";
    Write "dep_add"(DepArgs) => write::dep_add, "加一条依赖：before 完成后才能做 after。会成环时拒绝，并列出环上的事项。";
    Write "dep_remove"(DepArgs) => write::dep_remove, "删除一条依赖。";
    Write "apply"(ApplyArgs) => write::apply, "按计划批量新建文件夹、事项和依赖，全部成功或全部不生效。新事项用 ref 互相引用，已有事项用编号。先用 dry_run 预览给用户看。";
    Write "undo"(UndoArgs) => write::undo, "撤销一个批次（批次号见写操作的结果或 log）。撤销本身也是一个新批次。";
}

/// The folder at a path; `None` is the root.
fn find_folder(b: &Board, path: &str) -> Result<Option<FolderId>, ApiError> {
    b.find_folder(path).ok_or_else(|| ApiError::not_found(format!("未找到文件夹 /{}", path.trim().trim_matches('/'))))
}

fn find_task<'a>(b: &Board<'a>, number: i64) -> Result<&'a Task, ApiError> {
    b.task_by_number(number).ok_or_else(|| ApiError::not_found(format!("不存在事项 #{number}")))
}

/// Runs a tool. `id` makes a write idempotent: the same id returns the first result.
pub async fn call(st: &AppState, auth: Auth, name: &str, args: Value, id: Option<MutationId>) -> Result<Reply, ApiError> {
    let now = st.clock.now();
    match parse_call(name, args)? {
        // The read and the sequence number come from the same call on the one connection: no
        // change can come between them.
        Call::Read(run) => {
            st.db
                .call(move |c| {
                    let output = run(c, &Cx { owner: &auth.user_id, now })?;
                    Ok(Reply { output, seq: Some(store::latest_seq(c, &auth.user_id)?) })
                })
                .await
        }
        Call::Write(build) => {
            auth.require_write()?;
            let hub = st.hub.clone();
            st.db
                .call(move |c| {
                    let output = write::run(c, &auth, id, now, build)?;
                    hub.changed(&auth.user_id);
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
