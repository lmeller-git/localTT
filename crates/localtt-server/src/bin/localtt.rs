//! The composition root: resolve the data root, lock one campaign, open the
//! store, bootstrap the actor and run the TUI.

use std::{env, fs, io, path::PathBuf};

use clap::{Parser, Subcommand};
use localtt_adapter::store::{SqliteOptions, SqliteStore};
use localtt_core::types::PlayerId;
use localtt_engine::ports::store::StoreFault;
use localtt_server::{
    app::AppHandle,
    config::{CampaignLock, ConfigError, DataPaths},
    content,
    env::ServerEnv,
    session::{CreateCampaign, StartError, start},
    tui::{TuiError, install_panic_hook, run},
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
}

#[derive(Debug, Subcommand)]
enum Mode {
    /// Run the terminal frontend (the default).
    Tui,
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
    Tui(#[from] TuiError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[tokio::main]
async fn main() {
    install_panic_hook();
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Mode::Tui) | None => run_tui(cli).await,
    };
    if let Err(error) = result {
        eprintln!("localtt: {error}");
        std::process::exit(1);
    }
}

async fn run_tui(cli: Cli) -> Result<(), CliError> {
    let paths = DataPaths::resolve(cli.data.as_deref())?;
    paths.ensure()?;
    let campaign = cli.campaign.unwrap_or_else(|| paths.default_campaign());
    if let Some(parent) = campaign.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let _lock = CampaignLock::acquire(&campaign)?;

    let mut options = SqliteOptions::default();
    options.startup_backup = !cli.no_backup;
    let mut store = SqliteStore::open_with(&campaign, options)?;
    let create = cli.create.then(|| {
        CreateCampaign::new(
            cli.name.unwrap_or_else(|| "New Campaign".to_owned()),
            cli.dm.unwrap_or_else(default_dm_name),
        )
    });
    let started = start(&mut store, create.as_ref(), cli.actor)?;
    let runtime_content = content::load(&paths.reference);
    if let Some(warning) = runtime_content.warning {
        eprintln!("localtt: {warning}");
    }
    let app = AppHandle::start(
        store,
        started.snapshot,
        Box::new(ServerEnv::new(runtime_content.source)),
    )?;
    run(app, started.actor).await?;
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
