use std::fs::File;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};

use nexttodo_server::ai::{Ai, llm::Https, secret::SecretKey};
use nexttodo_server::{AppState, Clock, Db, app, auth, db, local, start, with_web};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about = "NextTodo server")]
struct Cli {
    /// Directory holding the database
    #[arg(long, env = "NEXTTODO_DATA", default_value = "data")]
    data: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the API, and the web app when given its files
    Serve {
        #[arg(long, env = "NEXTTODO_LISTEN", default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
        /// Directory with the built web app (`apps/web/dist`)
        #[arg(long, env = "NEXTTODO_WEB")]
        web: Option<PathBuf>,
    },
    /// Replace the database with a snapshot, the server stopped; the data replaced is kept beside it
    Restore { snapshot: PathBuf },
    /// Create an administrator account; asks for the password, or reads it from stdin
    CreateAdmin {
        username: String,
        #[arg(long)]
        display_name: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        // no colour codes in container logs
        .with_ansi(std::io::stdout().is_terminal())
        .init();
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.data)?;
    // From the environment alone, not an argument, which anyone on the machine could read.
    // Compose passes an empty one when `.env` has none.
    let secret = std::env::var("NEXTTODO_SECRET_KEY").ok().filter(|s| !s.is_empty()).map(|s| SecretKey::from_base64(&s)).transpose()?;
    let db = cli.data.join("nexttodo.db");
    match cli.command {
        Command::Serve { listen, web } => {
            let _lock = lock(&cli.data).await?;
            let ai = Ai::new(secret, Arc::new(Https));
            let (started, ()) = start::start(&db, cli.data.join("snapshots"), listen, ai, |_| Ok(())).await?;
            let router = match &web {
                Some(dir) => with_web(app(started.state.clone()), dir),
                None => app(started.state.clone()),
            };
            tracing::info!(%listen, "listening");
            started.serve(router).with_graceful_shutdown(shutdown()).await?;
            Ok(())
        }
        Command::Restore { snapshot } => {
            let _lock = lock(&cli.data).await?;
            match db::restore(&db, &snapshot, Clock::system().now())? {
                Some(kept) => println!("已恢复，被替换的数据在 {}", kept.display()),
                None => println!("已恢复"),
            }
            Ok(())
        }
        Command::CreateAdmin { username, display_name } => {
            let state = AppState::new(Db::open(&db)?, Clock::system());
            let password = read_password()?;
            let display_name = display_name.unwrap_or_else(|| username.clone());
            auth::create_admin(&state, username, display_name, password).await?;
            println!("已创建管理员");
            Ok(())
        }
    }
}

/// The data directory's lock, held until the process ends: a restore never replaces the database
/// under a server running on it.
async fn lock(data: &Path) -> anyhow::Result<File> {
    local::lock(&data.join("lock"), Duration::ZERO).await.map_err(|e| anyhow::anyhow!("{e}"))
}

/// Asks twice on a terminal; reads one line when the password is piped in (scripts, `docker exec` without `-t`).
fn read_password() -> anyhow::Result<String> {
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        return Ok(line.trim_end_matches(['\r', '\n']).to_owned());
    }
    let password = rpassword::prompt_password("密码：")?;
    if rpassword::prompt_password("再输一次：")? != password {
        anyhow::bail!("两次输入的密码不一样");
    }
    Ok(password)
}

/// Ctrl-C, or the SIGTERM a container gets when it stops; the server runs on Linux, and builds
/// elsewhere for the tests of local mode, which runs in the Windows app.
async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.expect("listen for Ctrl-C");
}
