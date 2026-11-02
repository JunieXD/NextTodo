mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::TestApp;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn pages_get_the_app_and_missing_files_get_404() {
    let dir = std::env::temp_dir().join(format!("nexttodo-web-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html>app").unwrap();
    std::fs::write(dir.join("assets/app-1a2b.js"), "console.log(1)").unwrap();
    let app = nexttodo_server::with_web(TestApp::new().app, &dir);
    let get = |path: &str| {
        let req = Request::get(path).body(Body::empty()).unwrap();
        let app = app.clone();
        async move {
            let res = app.oneshot(req).await.unwrap();
            let cache = res.headers().get(header::CACHE_CONTROL).map(|v| v.to_str().unwrap().to_owned());
            let kind = res.headers().get(header::CONTENT_TYPE).map(|v| v.to_str().unwrap().to_owned()).unwrap_or_default();
            let status = res.status();
            let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
            (status, kind, cache.unwrap_or_default(), body)
        }
    };

    let (status, _, cache, body) = get("/folders/019a4b2c-0000-7000-8000-000000000001?t=x").await;
    assert_eq!((status, cache.as_str(), body.as_str()), (StatusCode::OK, "no-cache", "<!doctype html>app"), "a page of the app");
    let (status, _, cache, _) = get("/assets/app-1a2b.js").await;
    assert_eq!((status, cache.as_str()), (StatusCode::OK, "public, max-age=31536000, immutable"));
    let (status, _, cache, _) = get("/assets/app-gone.js").await;
    assert_eq!((status, cache.as_str()), (StatusCode::NOT_FOUND, "no-cache"), "never cache a page in place of a missing asset");
    for path in ["/api", "/api/v2/me", "/mcp/x"] {
        let (status, kind, _, _) = get(path).await;
        assert_eq!((status, kind.as_str()), (StatusCode::NOT_FOUND, "application/json"), "{path}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
