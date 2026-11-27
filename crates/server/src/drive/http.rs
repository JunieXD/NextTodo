//! What the WebDAV and S3 clients share: the HTTP client, and what a failed request means.

use std::time::Duration;

use quick_xml::events::BytesRef;
use ureq::Timeout;
use ureq::http::Response;

use super::remote::Failed;

/// The largest file read from a drive: a snapshot of a large database, gzipped.
const MAX_READ: u64 = 512 * 1024 * 1024;

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        // WebDAV's MOVE, MKCOL and PROPFIND.
        .allow_non_standard_methods(true)
        // Every request goes to the drive the user gave, and nowhere it sends it on to.
        .max_redirects(0)
        // The commits read at once, each on a connection of its own.
        .max_idle_connections_per_host(super::AHEAD)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into()
}

/// A request that got no answer. Only one that never reached the drive is known not to have done
/// anything.
pub(crate) fn transport(e: ureq::Error) -> Failed {
    tracing::warn!("drive request failed: {e}");
    match e {
        ureq::Error::HostNotFound => Failed::Unsent("无法解析网盘的地址，请检查地址".into()),
        ureq::Error::ConnectionFailed | ureq::Error::ConnectProxyFailed(_) | ureq::Error::Timeout(Timeout::Resolve | Timeout::Connect) => {
            Failed::Unsent("无法连接网盘".into())
        }
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => Failed::Unsent("网盘的 HTTPS 证书无效，请检查地址".into()),
        ureq::Error::BadUri(_) | ureq::Error::Http(_) | ureq::Error::RequireHttpsOnly(_) => Failed::Unsent("网盘的地址无效".into()),
        _ => Failed::Unknown("网盘没有回应".into()),
    }
}

/// A request the drive answered with `status`, which is not what it should have: refused, so not
/// done, or failed on the drive's side, and maybe done.
pub(crate) fn refused(status: u16) -> Failed {
    match status {
        401 | 403 => Failed::Unsent("网盘账号或密码错误，或无权限".into()),
        429 | 503 => Failed::Unsent("网盘暂时限制了请求".into()),
        500.. => Failed::Unknown(format!("网盘出错（HTTP {status}）")),
        _ => Failed::Unsent(format!("网盘拒绝了请求（HTTP {status}）")),
    }
}

/// Folder `dir` as a path, ending in `/`; the drive's folder itself when empty.
pub(crate) fn folder(dir: &str) -> String {
    if dir.is_empty() { String::new() } else { format!("{dir}/") }
}

/// The whole body of `res`.
pub(crate) fn body(res: Response<ureq::Body>) -> Result<Vec<u8>, Failed> {
    res.into_body().with_config().limit(MAX_READ).read_to_vec().map_err(transport)
}

/// What a reference in XML text stands for: `&amp;` and the like, or a character by number. The text
/// around it comes apart from it, and is put together with it.
pub(crate) fn referred(r: &BytesRef) -> Result<String, Failed> {
    let unreadable = || Failed::Unknown("网盘的回复无法读取：XML 里有不认得的引用".into());
    if let Some(c) = r.resolve_char_ref().map_err(|_| unreadable())? {
        return Ok(c.to_string());
    }
    quick_xml::escape::resolve_predefined_entity(r).map(str::to_owned).ok_or_else(unreadable)
}

/// `s` as a part of a path or a query's value: all but the unreserved characters percent-encoded.
pub(crate) fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => char::from(b).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}
