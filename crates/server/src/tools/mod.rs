//! Tools for agents and scripts: one implementation behind the command line, MCP and
//! `/api/v1/tools`. See `docs/agent.md`.

mod args;
mod read;
mod text;
mod write;

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use nexttodo_core::board::Board;
use nexttodo_core::model::{FolderId, MutationId, Task};
use rmcp::handler::server::common::schema_for_type;
use rmcp::model::JsonObject;
use rusqlite::Connection;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::AppState;
use crate::auth::Auth;
use crate::error::{ApiError, Json};
use args::*;
use write::{Write, WriteCx};

/// What a tool returns: text for people and agents, data for programs.
#[derive(Debug, Serialize)]
pub struct Output {
    pub text: String,
    pub data: Value,
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
                    let args: $args = parse(args)?;
                    Ok(tools!(@call $kind, $run, args))
                })*
                _ => Err(ApiError::not_found(format!("没有叫 {name} 的工具"))),
            }
        }
    };
    (@call Read, $run:path, $args:ident) => { Call::Read(Box::new(move |c, cx| $run(c, cx, $args))) };
    (@call $kind:ident, $run:path, $args:ident) => { Call::Write(Box::new(move |w| $run(w, $args))) };
}

tools! {
    Read "tree"(NoArgs) => read::tree, "文件夹树，每个文件夹直接包含的待办数和可做数。";
    Read "list"(ListArgs) => read::list, "列出一个文件夹的子文件夹和事项，事项按先后顺序；默认不含已完成的。";
    Read "next"(NoArgs) => read::next, "接下来：所有现在就能做的事项（前置都已完成），有截止时间的排在前面。回答“现在该做什么”时用它。";
    Read "show"(ShowArgs) => read::show, "事项详情：状态、文件夹、截止时间和提醒、前置、后续、来源和描述。";
    Read "search"(SearchArgs) => read::search, "按关键词搜索事项的标题和描述，不区分大小写，最多 50 项。";
    Read "graph"(GraphArgs) => read::graph, "文件夹（含子文件夹）里的事项和依赖，按先后顺序列出，每项后面是它的前置；连到文件夹外的事项也列出。";
    Read "log"(LogArgs) => read::log, "最近的改动批次，新的在前。撤销时要用批次号。";
    Write "add"(AddArgs) => write::add, "添加一个事项；文件夹不存在时一起新建。一次加多个、带依赖的事项时用 apply。";
    Write "edit"(EditArgs) => write::edit, "修改事项的标题、描述、截止时间或提醒；只改给出的字段，due、remind 给 null 表示清除。";
    Write "done"(TasksArgs) => write::done, "把事项标为完成，结果里会列出因此变成可做的事项。";
    Write "reopen"(TasksArgs) => write::reopen, "重新打开已完成的事项。";
    Write "mkdir"(MkdirArgs) => write::mkdir, "新建文件夹，路径上缺的层级一起建。";
    Write "rename"(RenameArgs) => write::rename, "给文件夹改名。";
    Write "move"(MoveArgs) => write::move_to, "把事项或文件夹移到一个已有的文件夹，/ 是根目录。";
    Destructive "delete"(DeleteArgs) => write::delete, "删除事项或文件夹；文件夹连同里面的子文件夹和事项一起删除。可以用 undo 撤销。";
    Write "dep_add"(DepArgs) => write::dep_add, "加一条依赖：before 完成后才能做 after。会成环时拒绝，并列出环上的事项。";
    Write "dep_remove"(DepArgs) => write::dep_remove, "删除一条依赖。";
    Write "apply"(ApplyArgs) => write::apply, "按计划批量新建文件夹、事项和依赖，全部成功或全部不生效。新事项用 ref 互相引用，已有事项用编号。先用 dry_run 预览给用户看。";
    Write "undo"(UndoArgs) => write::undo, "撤销一个批次（批次号见写操作的结果或 log）。撤销本身也是一个新批次。";
}

/// The folder at a path; `None` is the root.
fn find_folder(b: &Board, path: &str) -> Result<Option<FolderId>, ApiError> {
    b.find_folder(path).ok_or_else(|| ApiError::not_found(format!("找不到文件夹 /{}", path.trim().trim_matches('/'))))
}

fn find_task<'a>(b: &Board<'a>, number: i64) -> Result<&'a Task, ApiError> {
    b.task_by_number(number).ok_or_else(|| ApiError::not_found(format!("没有 #{number} 这个事项")))
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, ApiError> {
    serde_path_to_error::deserialize(args).map_err(|e| {
        let path = e.path().to_string();
        ApiError::invalid(e.inner().to_string(), (path != ".").then_some(path))
    })
}

/// Runs a tool. `id` makes a write idempotent: the same id returns the first result.
pub async fn call(st: &AppState, auth: Auth, name: &str, args: Value, id: Option<MutationId>) -> Result<Output, ApiError> {
    let now = st.clock.now();
    match parse_call(name, args)? {
        Call::Read(run) => st.db.call(move |c| run(c, &Cx { owner: &auth.user_id, now })).await,
        Call::Write(build) => {
            auth.require_write()?;
            st.db.call(move |c| write::run(c, &auth, id, now, build)).await
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
) -> Result<Json<Output>, ApiError> {
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
