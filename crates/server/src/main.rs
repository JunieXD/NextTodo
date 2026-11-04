use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use std::sync::Arc;

use nexttodo_server::ai::{Ai, llm::Https, run_queue, secret::SecretKey};
use nexttodo_server::{AppState, Clock, Db, MUTATION_RETENTION_MS, app, auth, db, with_web};
use tokio::signal::unix::{SignalKind, signal};
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
    let state = AppState::new(Db::open(&cli.data.join(DB_FILE))?, Clock::system()).with_ai(Ai::new(secret, Arc::new(Https)));
    match cli.command {
        Command::Serve { listen, web } => serve(state, listen, web, cli.data).await,
        Command::CreateAdmin { username, display_name } => {
            let password = read_password()?;
            let display_name = display_name.unwrap_or_else(|| username.clone());
            auth::create_admin(&state, username, display_name, password).await?;
            println!("已创建管理员");
            Ok(())
        }
    }
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

async fn serve(state: AppState, listen: SocketAddr, web: Option<PathBuf>, data: PathBuf) -> anyhow::Result<()> {
    tokio::spawn(daily(state.clone(), data));
    tokio::spawn(run_queue(state.clone()));
    let router = match &web {
        Some(dir) => with_web(app(state), dir),
        None => app(state),
    };
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "listening");
    axum::serve(listener, router).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

const DB_FILE: &str = "nexttodo.db";

/// How many daily snapshots of the database are kept.
const SNAPSHOTS_KEPT: usize = 7;

/// Once at start and then every day: clears the old part of the mutation log and snapshots the
/// database into `data/snapshots`. A failure is logged and tried again the next day.
async fn daily(state: AppState, data: PathBuf) {
    let mut tick = tokio::time::interval(Duration::from_secs(24 * 3600));
    loop {
        tick.tick().await;
        let cutoff = state.clock.now() - MUTATION_RETENTION_MS;
        // a failure is already logged by the error conversion
        if let Ok(cleared) = state.db.call(move |c| Ok(db::prune(c, cutoff)?)).await {
            tracing::info!(cleared, "pruned mutation log");
        }
        let (data, now) = (data.clone(), state.clock.now());
        match tokio::task::spawn_blocking(move || db::snapshot(&data.join(DB_FILE), &data.join("snapshots"), now, SNAPSHOTS_KEPT))
            .await
            .expect("snapshot task panicked")
        {
            Ok(path) => tracing::info!(path = %path.display(), "snapshot written"),
            Err(e) => tracing::error!("snapshot failed: {e:#}"),
        }
    }
}

async fn shutdown() {
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
