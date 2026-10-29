//! The composition root: resolve the data root, lock one campaign, open the
//! store, bootstrap the actor and run the selected frontend.

use std::{
    env,
    fs,
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{Parser, Subcommand};
use localtt_adapter::store::{AsyncSqliteStore, SqliteOptions, SqliteStore};
use localtt_core::types::{PlayerId, TokenHash};
use localtt_engine::{commands::Command, ports::store::StoreFault};
#[cfg(feature = "tui")]
use localtt_server::tui::{TuiError, install_panic_hook, run as run_tui_screen};
use localtt_server::{
    app::AppHandle,
    config::{CampaignLock, ConfigError, DataPaths, StagedSecret, dm_key_path, read_secret},
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
    /// Do not keep a DM key file; generate a new key on every start.
    #[arg(long, global = true)]
    no_dm_key_file: bool,
    /// Replace the DM key file and the DM seat's token hash.
    #[arg(long, global = true, conflicts_with = "no_dm_key_file")]
    rotate_dm_key: bool,
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
    campaign: PathBuf,
    dm_hash: Option<TokenHash>,
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
    let dm_hash = started
        .snapshot
        .campaign
        .players
        .iter()
        .find(|player| player.id == started.actor)
        .and_then(|player| player.token_hash.clone());
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
        campaign,
        dm_hash,
        _lock: lock,
    })
}

async fn start_http(session: &Session, cli: &Cli) -> Result<ServerHandle, CliError> {
    let token = resolve_dm_key(session, cli).await?;
    let mut config = ServerConfig::new(cli.bind)
        .with_origins(cli.origins.clone())
        .with_diagnostics(cli.diagnostics);
    if let Some(web_dir) = &cli.web_dir {
        config = config.with_web_dir(web_dir);
    }
    let server = serve(session.app.clone(), config).await?;
    let base = format!("http://{}", server.local_addr());
    eprintln!("localtt: DM token: {}", token.expose());
    if cli.web_dir.is_some() {
        eprintln!("localtt: DM link: {base}/#token={}", token.expose());
    } else {
        eprintln!("localtt: API only; pass --web-dir to serve the browser client");
    }
    eprintln!("localtt: HTTP listening on {base}");
    Ok(server)
}

/// Resolves the selected DM's bearer token and keeps the roster hash in sync.
///
/// A matching key file is reused. A mismatch, or a missing file while the seat
/// already has a hash, is an error unless `--rotate-dm-key` was passed. With no
/// key file and no seat hash, a pre-M5 `<data>/server.token` is adopted once.
async fn resolve_dm_key(session: &Session, cli: &Cli) -> Result<ServerToken, CliError> {
    if cli.no_dm_key_file {
        let token = ServerToken::generate();
        commit_dm_hash(session, &token).await?;
        return Ok(token);
    }

    let path = dm_key_path(&session.campaign);
    let file = read_secret(&path)?;
    match plan_dm_key(
        file.as_deref(),
        session.dm_hash.as_ref(),
        cli.rotate_dm_key,
        &path,
    )? {
        KeyPlan::Reuse(token) => Ok(token),
        KeyPlan::Adopt(token) => {
            commit_dm_hash(session, &token).await?;
            Ok(token)
        }
        KeyPlan::Generate => {
            let legacy = session.paths.server_token();
            let adopted = match read_secret(&legacy)? {
                Some(secret) if !secret.trim().is_empty() => {
                    Some(ServerToken::from_secret(secret.trim()))
                }
                _ => None,
            };
            let token = adopted.clone().unwrap_or_else(ServerToken::generate);
            let staged = StagedSecret::write(&path, token.expose())?;
            commit_dm_hash(session, &token).await?;
            if let Err(error) = staged.commit() {
                eprintln!(
                    "localtt: the DM key hash is committed but {} was not published; rerun \
                     with --rotate-dm-key to rewrite it",
                    path.display()
                );
                return Err(error.into());
            }
            if adopted.is_some() && legacy.is_file() {
                let target = session.paths.migrated_server_token();
                match fs::rename(&legacy, &target) {
                    Ok(()) => eprintln!(
                        "localtt: adopted {} as the DM key and moved it to {}",
                        legacy.display(),
                        target.display()
                    ),
                    Err(error) => eprintln!(
                        "localtt: adopted {} as the DM key but could not rename it: {error}",
                        legacy.display()
                    ),
                }
            }
            Ok(token)
        }
    }
}

/// What startup should do with the DM key file.
#[derive(Debug)]
enum KeyPlan {
    /// The file matches the seat; keep both.
    Reuse(ServerToken),
    /// The seat has no hash; adopt the file's token.
    Adopt(ServerToken),
    /// Mint a token and publish a new file.
    Generate,
}

/// Chooses between reusing, adopting and replacing the DM key file.
///
/// A missing file with an existing seat hash fails closed; only an explicit
/// rotation may replace it.
fn plan_dm_key(
    file: Option<&str>,
    seat: Option<&TokenHash>,
    rotate: bool,
    path: &Path,
) -> Result<KeyPlan, ConfigError> {
    if rotate {
        return Ok(KeyPlan::Generate);
    }
    let Some(secret) = file else {
        return if seat.is_some() {
            Err(ConfigError::DmKeyMissing {
                path: path.to_path_buf(),
            })
        } else {
            Ok(KeyPlan::Generate)
        };
    };
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(ConfigError::DmKeyEmpty {
            path: path.to_path_buf(),
        });
    }
    let token = ServerToken::from_secret(secret);
    match seat {
        Some(hash) if *hash == token.hash() => Ok(KeyPlan::Reuse(token)),
        Some(_) => Err(ConfigError::DmKeyMismatch {
            path: path.to_path_buf(),
        }),
        None => Ok(KeyPlan::Adopt(token)),
    }
}

async fn commit_dm_hash(session: &Session, token: &ServerToken) -> Result<(), CliError> {
    session
        .app
        .execute(
            session.actor,
            Command::RotateDmKey {
                player: session.actor,
                token_hash: token.hash(),
            },
        )
        .await?;
    Ok(())
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
            "--no-dm-key-file",
        ])
        .expect("the CLI parses");
        assert!(matches!(cli.command, Some(Mode::Serve)));
        assert_eq!(cli.data, Some(PathBuf::from("/tmp/localtt")));
        assert_eq!(cli.origins, vec!["http://localhost:5173"]);
        assert_eq!(cli.web_dir, Some(PathBuf::from("web/dist")));
        assert!(cli.no_dm_key_file);
        assert!(!cli.rotate_dm_key);
    }

    #[test]
    fn dm_key_flags_are_exclusive() {
        let error =
            Cli::try_parse_from(["localtt", "serve", "--no-dm-key-file", "--rotate-dm-key"])
                .expect_err("regenerating and rotating at once is ambiguous");
        assert!(error.to_string().contains("cannot be used with"));
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

    fn key_path() -> PathBuf {
        PathBuf::from("/data/campaigns/default.sqlite.dm.key")
    }

    #[test]
    fn a_matching_key_file_is_reused() {
        // Arrange
        let token = ServerToken::from_secret("dm-secret");
        let hash = token.hash();

        // Act
        let plan = plan_dm_key(Some("dm-secret"), Some(&hash), false, &key_path())
            .expect("the file matches the seat");

        // Assert
        assert!(matches!(plan, KeyPlan::Reuse(reused) if reused.expose() == "dm-secret"));
    }

    #[test]
    fn a_mismatched_key_file_is_refused_without_rotation() {
        // Arrange
        let seat = ServerToken::from_secret("seat-secret").hash();

        // Act
        let result = plan_dm_key(Some("stale-secret"), Some(&seat), false, &key_path());

        // Assert
        assert!(matches!(
            result,
            Err(ConfigError::DmKeyMismatch { path }) if path == key_path()
        ));
    }

    #[test]
    fn a_missing_key_file_with_a_seat_hash_is_refused() {
        // Arrange
        let seat = ServerToken::from_secret("seat-secret").hash();

        // Act
        let result = plan_dm_key(None, Some(&seat), false, &key_path());

        // Assert
        assert!(matches!(
            result,
            Err(ConfigError::DmKeyMissing { path }) if path == key_path()
        ));
    }

    #[test]
    fn a_key_file_without_a_seat_hash_is_adopted() {
        // Act
        let plan = plan_dm_key(Some("dm-secret"), None, false, &key_path())
            .expect("a fresh seat adopts the file");

        // Assert
        assert!(matches!(plan, KeyPlan::Adopt(token) if token.expose() == "dm-secret"));
    }

    #[test]
    fn an_empty_key_file_is_refused() {
        // Act
        let result = plan_dm_key(Some("   \n"), None, false, &key_path());

        // Assert
        assert!(matches!(
            result,
            Err(ConfigError::DmKeyEmpty { path }) if path == key_path()
        ));
    }

    #[test]
    fn rotation_generates_even_when_the_file_matches() {
        // Arrange
        let seat = ServerToken::from_secret("dm-secret").hash();

        // Act
        let plan = plan_dm_key(Some("dm-secret"), Some(&seat), true, &key_path())
            .expect("rotation is always allowed");

        // Assert
        assert!(matches!(plan, KeyPlan::Generate));
    }

    #[test]
    fn a_missing_file_and_hash_generates_a_new_key() {
        // Act
        let plan = plan_dm_key(None, None, false, &key_path()).expect("a fresh campaign generates");

        // Assert
        assert!(matches!(plan, KeyPlan::Generate));
    }
}
