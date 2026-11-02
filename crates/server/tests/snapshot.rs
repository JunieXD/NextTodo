use nexttodo_server::{AppState, Clock, Db, auth, db};
use rusqlite::Connection;

#[tokio::test]
async fn snapshots_hold_the_data_and_only_the_newest_are_kept() {
    let dir = std::env::temp_dir().join(format!("nexttodo-snapshot-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nexttodo.db");
    let state = AppState::new(Db::open(&file).unwrap(), Clock::system());
    auth::create_admin(&state, "junie".into(), "Junie".into(), "correct horse".into()).await.unwrap();
    let snapshots = dir.join("snapshots");
    // A partial copy left by a crash doesn't get in the way.
    std::fs::create_dir_all(&snapshots).unwrap();
    std::fs::write(snapshots.join("snapshot-0000000009.db.partial"), "half").unwrap();

    let mut last = None;
    for day in 1..=9 {
        last = Some(db::snapshot(&file, &snapshots, day * 86_400_000, 7).unwrap());
    }

    let mut names: Vec<String> = std::fs::read_dir(&snapshots).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    let expected: Vec<String> = (3..=9).map(|day| format!("snapshot-{:010}.db", day * 86_400)).collect();
    assert_eq!(names, expected);
    let copy = Connection::open(last.unwrap()).unwrap();
    let user: String = copy.query_row("SELECT username FROM users", [], |r| r.get(0)).unwrap();
    assert_eq!(user, "junie");
    std::fs::remove_dir_all(dir).unwrap();
}
