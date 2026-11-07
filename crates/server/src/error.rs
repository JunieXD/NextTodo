//! The error shape of the API (`{code, message, details?}`) and a JSON extractor that reports
//! which field was wrong.

use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use nexttodo_core::model::TaskId;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use ts_rs::TS;

/// How errors look in responses and sync messages.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub details: Option<Value>,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError { status, code, message: message.into(), details: None }
    }

    pub fn invalid(message: impl Into<String>, field: Option<String>) -> Self {
        let mut e = Self::new(StatusCode::BAD_REQUEST, "invalid", message);
        e.details = field.map(|f| serde_json::json!({ "field": f }));
        e
    }

    pub fn unauthenticated(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthenticated", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    /// `path` goes around the cycle and ends where it starts; `titles` spells it out for the user.
    pub fn cycle(path: Vec<TaskId>, titles: &str) -> Self {
        let mut e = Self::new(StatusCode::CONFLICT, "cycle", format!("添加该依赖将形成循环：{titles}"));
        e.details = Some(serde_json::json!({ "path": path }));
        e
    }

    pub fn folder_cycle() -> Self {
        Self::new(StatusCode::CONFLICT, "folder_cycle", "文件夹不能移入自身或其子文件夹")
    }

    /// A model's API failed to answer as asked.
    pub fn upstream(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, "upstream", message)
    }

    pub fn resync() -> Self {
        Self::new(StatusCode::GONE, "resync", "本地数据已过期，需要重新下载")
    }
}

/// What to tell the user of `e`, which made `what` fail, as docs/ui.md#文案 has it: `what` failed,
/// then why in words for them: the messages in `e` written for the user, in Chinese as all such are
/// (AGENTS.md), and what the system's own words after them mean, for the common failures of files.
/// Without any, that it may be tried again. The system's words go to the log, where they help: to
/// the user they mean little.
pub fn for_user(what: &str, e: &(dyn std::error::Error + 'static)) -> String {
    let chain = || std::iter::successors(Some(e), |e| e.source());
    tracing::warn!("{what}：{}", chain().map(ToString::to_string).collect::<Vec<_>>().join(": "));
    let mut why: Vec<String> = chain().map(ToString::to_string).take_while(|m| chinese(m)).collect();
    why.extend(chain().filter(|e| !chinese(&e.to_string())).find_map(meaning).map(str::to_owned));
    if why.is_empty() { format!("{what}失败，请稍后重试") } else { format!("{what}失败：{}", why.join("：")) }
}

fn chinese(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// What a failure of files means, in words for the user.
fn meaning(e: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    use std::io::ErrorKind;
    if let Some(e) = e.downcast_ref::<std::io::Error>() {
        return match e.kind() {
            ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => Some("没有权限访问这个位置"),
            ErrorKind::StorageFull | ErrorKind::QuotaExceeded => Some("存储空间不足"),
            ErrorKind::NotFound => Some("找不到文件"),
            _ => None,
        };
    }
    match e.downcast_ref::<rusqlite::Error>()?.sqlite_error_code()? {
        rusqlite::ErrorCode::DiskFull => Some("存储空间不足"),
        rusqlite::ErrorCode::ReadOnly | rusqlite::ErrorCode::PermissionDenied => Some("没有权限访问这个位置"),
        _ => None,
    }
}

impl ApiError {
    /// The message, with the field it is about when there is one.
    pub fn describe(&self) -> String {
        match self.details.as_ref().and_then(|d| d["field"].as_str()) {
            Some(field) => format!("{}（{field}）", self.message),
            None => self.message.clone(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        tracing::error!(error = %e, "database error");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "服务器错误，请稍后重试")
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        tracing::error!(error = %e, "stored JSON error");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "服务器错误，请稍后重试")
    }
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody { code: self.code, message: self.message.clone(), details: self.details.clone() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, axum::Json(self.body())).into_response()
    }
}

/// Reads a value that came from outside; bad input becomes a 400 naming the field.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_path_to_error::deserialize(value).map_err(|e| {
        let path = e.path().to_string();
        ApiError::invalid(e.inner().to_string(), (path != ".").then_some(path))
    })
}

/// JSON body extractor and response. Bad input becomes a 400 naming the field, like `ops[0].due`.
///
/// Like axum's own, it insists on `Content-Type: application/json`: a cross-site form cannot send
/// that, which keeps other sites from posting to the API with the user's cookie.
pub struct Json<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Json<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        let is_json = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(';').next().is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json")));
        if !is_json {
            return Err(ApiError::invalid("请求体须为 JSON，并设置 Content-Type: application/json", None));
        }
        let bytes = Bytes::from_request(req, state).await.map_err(|e| ApiError::invalid(e.body_text(), None))?;
        let de = &mut serde_json::Deserializer::from_slice(&bytes);
        let value = serde_path_to_error::deserialize(&mut *de).map_err(|e| {
            let path = e.path().to_string();
            let message = e.inner().to_string();
            // serde_json appends the position, which means nothing to the user
            let message = message.rsplit_once(" at line ").map_or(message.as_str(), |(m, _)| m).to_owned();
            ApiError::invalid(message, (path != ".").then_some(path))
        })?;
        de.end().map_err(|_| ApiError::invalid("请求体在 JSON 之后含有多余内容", None))?;
        Ok(Json(value))
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_user_ours_not_the_systems() {
        let denied = || std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied (os error 13)");
        let ours = anyhow::Error::from(denied()).context("备份失败");
        let told = for_user("恢复", ours.as_ref());
        assert!(told.starts_with("恢复失败：备份失败：") && !told.contains("os error"), "{told}");
        let bare = for_user("读取备份", &std::io::Error::other("unexpected end of file"));
        assert!(bare.starts_with("读取备份失败") && !bare.contains("unexpected"), "{bare}");
        let written = anyhow::anyhow!("这不是 NextTodo 的备份，或者文件已损坏");
        assert_eq!(for_user("读取备份", written.as_ref()), format!("读取备份失败：{written}"));
    }
}
