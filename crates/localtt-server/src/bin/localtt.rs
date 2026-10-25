//! The composition root: resolve the data root, lock one campaign, open the
//! store, bootstrap the actor and run the selected frontend.

use std::{env, fs, io, net::SocketAddr, path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use localtt_adapter::store::{AsyncSqliteStore, SqliteOptions, SqliteStore};
use localtt_core::types::PlayerId;
use localtt_engine::ports::store::StoreFault;
#[cfg(feature = "tui")]
use localtt_server::tui::{TuiError, install_panic_hook, run as run_tui_screen};
use localtt_server::{
    app::AppHandle,
    config::{CampaignLock, ConfigError, DataPaths, write_server_token},
    content,
    env::ServerEnv,
    error::AppError,
    http::{ServeError, ServerConfig, ServerHandle, ServerToken, serve},
    session::{CreateCampaign, StartError, start},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "localtt",
    version,
    about = "Local table-top campaign server and TUI"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Mode>,
    /// Campaign store path; defaults to `<data>/campaigns/default.sqlite`.
    #[arg(long, global = true)]
    campaign: Option<PathBuf>,
    /// Data root; defaults to the platform local data directory.
    #[arg(long, global = true)]
    data: Option<PathBuf>,
    /// Create a campaign in an empty store.
    #[arg(long, global = true)]
    create: bool,
    /// Display name for a newly created campaign.
    #[arg(long, global = true, requires = "create")]
    name: Option<String>,
    /// Display name for the DM seat of a newly created campaign.
    #[arg(long, global = true, requires = "create")]
    dm: Option<String>,
    /// Explicit roster member to act as; defaults to the sole DM.
    #[arg(long, global = true, value_parser = parse_actor)]
    actor: Option<PlayerId>,
    /// Skip the startup backup.
    #[arg(long, global = true)]
    no_backup: bool,
    /// HTTP bind address for `serve` or `tui --serve`.
    #[arg(long, global = true, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
    /// Extra browser origin allowed by the HTTP adapter; repeatable.
    #[arg(long = "origin", global = true, value_name = "URL")]
    origins: Vec<String>,
    /// Serve the built browser client from this directory; omit for API only.
    #[arg(long, global = true, value_name = "DIR")]
    web_dir: Option<PathBuf>,
    /// Expose authenticated actor and queue diagnostics.
    #[arg(long, global = true)]
    diagnostics: bool,
    /// Do not write the per-run token to `<data>/server.token`.
    #[arg(long, global = true)]
    no_token_file: bool,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum Mode {
    /// Run the terminal frontend (the default when built with the TUI).
    #[cfg(feature = "tui")]
    Tui {
        /// Also serve the HTTP API.
        #[arg(long)]
        serve: bool,
    },
    /// Run the headless HTTP API.
    Serve,
}

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Store(#[from] StoreFault),
    #[error(transparent)]
    Start(#[from] StartError),
    #[error(transparent)]
    App(#[from] AppError),
    #[error(transparent)]
    Http(#[from] ServeError),
    #[cfg(feature = "tui")]
    #[error(transparent)]
    Tui(#[from] TuiError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

struct Session {
    app: AppHandle,
    actor: PlayerId,
    paths: DataPaths,
    _lock: CampaignLock,
}

#[tokio::main]
async fn main() {
    init_tracing();
    #[cfg(feature = "tui")]
    install_panic_hook();
    let cli = Cli::parse();
    let result = match cli.command {
        #[cfg(feature = "tui")]
        Some(Mode::Tui { serve }) => run_tui(cli, serve).await,
        Some(Mode::Serve) => run_serve(cli).await,
        #[cfg(feature = "tui")]
        None => run_tui(cli, false).await,
        #[cfg(not(feature = "tui"))]
        None => run_serve(cli).await,
    };
    if let Err(error) = result {
        eprintln!("localtt: {error}");
        std::process::exit(1);
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("localtt_server=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[cfg(feature = "tui")]
async fn run_tui(cli: Cli, serve_http: bool) -> Result<(), CliError> {
    let session = open_session(&cli)?;
    let server = if serve_http {
        match start_http(&session, &cli).await {
            Ok(server) => Some(server),
            Err(error) => {
                if let Err(shutdown) = session.app.shutdown().await {
                    eprintln!("localtt: shutdown after HTTP failure: {shutdown}");
                }
                return Err(error);
            }
        }
    } else {
        None
    };

    let result = run_tui_screen(session.app.clone(), session.actor).await;
    if let Some(server) = server {
        server.stop();
    }
    result?;
    Ok(())
}

async fn run_serve(cli: Cli) -> Result<(), CliError> {
    let session = open_session(&cli)?;
    let server = match start_http(&session, &cli).await {
        Ok(server) => server,
        Err(error) => {
            if let Err(shutdown) = session.app.shutdown().await {
                eprintln!("localtt: shutdown after HTTP failure: {shutdown}");
            }
            return Err(error);
        }
    };

    tokio::signal::ctrl_c().await?;
    server.stop();
    session.app.shutdown().await?;
    Ok(())
}

fn open_session(cli: &Cli) -> Result<Session, CliError> {
    let paths = DataPaths::resolve(cli.data.as_deref())?;
    paths.ensure()?;
    let campaign = cli
        .campaign
        .clone()
        .unwrap_or_else(|| paths.default_campaign());
    if let Some(parent) = campaign.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let lock = CampaignLock::acquire(&campaign)?;

    let mut options = SqliteOptions::default();
    options.startup_backup = !cli.no_backup;
    let mut store = SqliteStore::open_with(&campaign, options)?;
    let create = cli.create.then(|| {
        CreateCampaign::new(
            cli.name
                .clone()
                .unwrap_or_else(|| "New Campaign".to_owned()),
            cli.dm.clone().unwrap_or_else(default_dm_name),
        )
    });
    let started = start(&mut store, create.as_ref(), cli.actor)?;
    let runtime_content = content::load(&paths.reference);
    if let Some(warning) = runtime_content.warning {
        eprintln!("localtt: {warning}");
    }
    let content = runtime_content.source;
    let app = AppHandle::start(
        AsyncSqliteStore::new(store),
        started.snapshot,
        Box::new(ServerEnv::new(Arc::clone(&content))),
        content,
    )?;
    Ok(Session {
        app,
        actor: started.actor,
        paths,
        _lock: lock,
    })
}

async fn start_http(session: &Session, cli: &Cli) -> Result<ServerHandle, CliError> {
    let token = ServerToken::generate();
    let mut config = ServerConfig::new(cli.bind, token.clone())
        .with_origins(cli.origins.clone())
        .with_diagnostics(cli.diagnostics);
    if let Some(web_dir) = &cli.web_dir {
        config = config.with_web_dir(web_dir);
    }
    let server = serve(session.app.clone(), session.actor, config).await?;
    if !cli.no_token_file {
        let path = session.paths.server_token();
        match write_server_token(&path, token.expose()) {
            Ok(()) => eprintln!("localtt: token written to {}", path.display()),
            Err(error) => eprintln!("localtt: could not write the token file: {error}"),
        }
    }
    eprintln!("localtt: bearer token: {}", token.expose());
    eprintln!("localtt: HTTP listening on http://{}", server.local_addr());
    Ok(server)
}

fn parse_actor(raw: &str) -> Result<PlayerId, String> {
    Uuid::parse_str(raw)
        .map(PlayerId::from_uuid)
        .map_err(|_| "actor must be a UUID".to_owned())
}

fn default_dm_name() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "DM".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_accepts_global_options_after_the_subcommand() {
        let cli = Cli::try_parse_from([
            "localtt",
            "serve",
            "--data",
            "/tmp/localtt",
            "--origin",
            "http://localhost:5173",
            "--web-dir",
            "web/dist",
            "--no-token-file",
        ])
        .expect("the CLI parses");
        assert!(matches!(cli.command, Some(Mode::Serve)));
        assert_eq!(cli.data, Some(PathBuf::from("/tmp/localtt")));
        assert_eq!(cli.origins, vec!["http://localhost:5173"]);
        assert_eq!(cli.web_dir, Some(PathBuf::from("web/dist")));
        assert!(cli.no_token_file);
    }

    #[test]
    fn a_name_requires_create() {
        let error = Cli::try_parse_from(["localtt", "serve", "--name", "Vault"])
            .expect_err("name alone is invalid");
        assert!(error.to_string().contains("--create"));
    }

    #[test]
    fn the_default_bind_is_loopback() {
        let cli = Cli::try_parse_from(["localtt", "serve"]).expect("the CLI parses");
        assert_eq!(
            cli.bind,
            "127.0.0.1:8787".parse().expect("a socket address")
        );
        assert!(cli.bind.ip().is_loopback());
    }

    #[cfg(feature = "tui")]
    #[test]
    fn tui_serve_is_an_explicit_mode_flag() {
        let cli = Cli::try_parse_from(["localtt", "tui", "--serve"]).expect("the CLI parses");
        assert!(matches!(cli.command, Some(Mode::Tui { serve: true })));
    }

    #[test]
    fn actor_parsing_rejects_non_uuids() {
        assert!(parse_actor("not-a-uuid").is_err());
        let id = Uuid::new_v4();
        assert_eq!(
            parse_actor(&id.to_string()).expect("a UUID parses"),
            PlayerId::from_uuid(id)
        );
    }
}
