//! The composition root: resolve the data root, lock one campaign, open the
//! store, bootstrap the actor and run the selected frontend.

use std::{
    env,
    fs,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
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
    /// HTTP bind address for `serve` or `tui --serve`; defaults to loopback, or
    /// `0.0.0.0:8787` with `--lan`.
    #[arg(long, global = true)]
    bind: Option<SocketAddr>,
    /// Bind a non-loopback address and allow detected LAN origins.
    #[arg(long, global = true)]
    lan: bool,
    /// Pinned LAN host (host or host:port) for invite links; requires `--lan`.
    #[arg(long, global = true, value_name = "HOST", requires = "lan")]
    lan_host: Option<String>,
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
    #[error("{0}")]
    Bind(String),
}

struct Session {
    app: AppHandle,
    actor: PlayerId,
    paths: DataPaths,
    campaign: PathBuf,
    dm_hash: Option<TokenHash>,
    _lock: CampaignLock,
}

impl Cli {
    /// Resolves the effective bind address under the `--lan` rule.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::Bind`] for a non-loopback bind without `--lan`, or a
    /// loopback bind with `--lan`.
    fn bind_address(&self) -> Result<SocketAddr, CliError> {
        const LOOPBACK: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8787);
        const ANY: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8787);
        match (self.lan, self.bind) {
            (false, None) => Ok(LOOPBACK),
            (false, Some(address)) if address.ip().is_loopback() => Ok(address),
            (false, Some(_)) => Err(CliError::Bind(
                "a non-loopback --bind requires --lan".to_owned(),
            )),
            (true, None) => Ok(ANY),
            (true, Some(address)) if address.ip().is_loopback() => Err(CliError::Bind(
                "--lan requires a non-loopback --bind".to_owned(),
            )),
            (true, Some(address)) => Ok(address),
        }
    }
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
    let bind = cli.bind_address()?;
    let mut lan_ips = if cli.lan {
        lan_addresses()?
    } else {
        Vec::new()
    };
    if !bind.ip().is_loopback() && !bind.ip().is_unspecified() {
        prefer_address(&mut lan_ips, Some(bind.ip()));
    }
    let lan_host = cli
        .lan_host
        .as_deref()
        .map(|host| normalize_lan_host(host, bind.port()))
        .transpose()
        .map_err(CliError::Bind)?;
    if cli.lan && lan_ips.is_empty() {
        eprintln!("localtt: --lan found no non-loopback interface addresses");
    }
    let mut config = ServerConfig::new(bind)
        .with_origins(cli.origins.clone())
        .with_diagnostics(cli.diagnostics)
        .with_lan(lan_host.clone(), lan_ips.clone());
    if let Some(web_dir) = &cli.web_dir {
        config = config.with_web_dir(web_dir);
    }
    let server = serve(session.app.clone(), config).await?;
    print_banner(cli, &server, &token, lan_host.as_deref(), &lan_ips);
    Ok(server)
}

fn print_banner(
    cli: &Cli,
    server: &ServerHandle,
    token: &ServerToken,
    lan_host: Option<&str>,
    lan_ips: &[IpAddr],
) {
    let bound = server.local_addr();
    let local = loopback_socket(bound);
    let dm_base = lan_host.map_or_else(
        || format!("http://{local}"),
        |host| format!("http://{host}"),
    );
    eprintln!("localtt: DM token: {}", token.expose());
    if cli.web_dir.is_some() {
        eprintln!("localtt: DM link: {dm_base}/#token={}", token.expose());
        for url in player_urls(lan_host, lan_ips, bound.port()) {
            eprintln!("localtt: player URL: {url}/");
        }
        if lan_host.is_none() && lan_ips.is_empty() {
            if bound.ip().is_loopback() {
                eprintln!(
                    "localtt: invite links point at this machine only; \
                     pass --lan to invite other devices"
                );
            } else {
                eprintln!(
                    "localtt: no non-loopback interface address was found; \
                     invite links fall back to loopback"
                );
            }
        }
    } else {
        eprintln!("localtt: API only; pass --web-dir to serve the browser client");
    }
    eprintln!("localtt: HTTP listening on http://{local} (bound {bound})");
    if !bound.ip().is_loopback() {
        eprintln!(
            "localtt: LAN binding is cleartext HTTP/WebSocket; use only on a trusted network"
        );
    }
}

fn loopback_socket(bound: SocketAddr) -> SocketAddr {
    let ip = if bound.is_ipv6() {
        IpAddr::V6(Ipv6Addr::LOCALHOST)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };
    SocketAddr::new(ip, bound.port())
}

/// The player URLs printed at startup, in invite-preference order.
fn player_urls(lan_host: Option<&str>, lan_ips: &[IpAddr], port: u16) -> Vec<String> {
    let mut urls = Vec::with_capacity(lan_ips.len() + 1);
    if let Some(host) = lan_host {
        urls.push(format!("http://{host}"));
    }
    for ip in lan_ips {
        let url = format!("http://{}", SocketAddr::new(*ip, port));
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

/// Normalizes `--lan-host` to `host:port` form.
///
/// # Errors
///
/// Rejects a scheme, a path, whitespace, an empty host or a malformed port.
fn normalize_lan_host(raw: &str, default_port: u16) -> Result<String, String> {
    let host = raw.trim();
    if host.is_empty() {
        return Err("--lan-host must not be empty".to_owned());
    }
    if host.contains("://") || host.contains('/') || host.chars().any(char::is_whitespace) {
        return Err("--lan-host must be a host or host:port without a scheme or path".to_owned());
    }
    if let Ok(address) = host.parse::<SocketAddr>() {
        return Ok(address.to_string());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, default_port).to_string());
    }
    if let Some(rest) = host.strip_prefix('[')
        && let Some((ip, tail)) = rest.split_once(']')
    {
        let ip: Ipv6Addr = ip
            .parse()
            .map_err(|_| "--lan-host has a malformed IPv6 address".to_owned())?;
        let port = if tail.is_empty() {
            default_port
        } else {
            tail.strip_prefix(':')
                .and_then(|port| port.parse().ok())
                .ok_or_else(|| "--lan-host has a malformed port".to_owned())?
        };
        return Ok(SocketAddr::new(IpAddr::V6(ip), port).to_string());
    }
    if let Some((name, port)) = host.rsplit_once(':') {
        if name.is_empty() || port.parse::<u16>().is_err() {
            return Err("--lan-host has a malformed host or port".to_owned());
        }
        return Ok(host.to_owned());
    }
    Ok(format!("{host}:{default_port}"))
}

/// Detected non-loopback interface addresses, ascending and deduplicated.
fn lan_addresses() -> io::Result<Vec<IpAddr>> {
    let mut addresses: Vec<IpAddr> = if_addrs::get_if_addrs()?
        .into_iter()
        .map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(address) => IpAddr::V4(address.ip),
            if_addrs::IfAddr::V6(address) => IpAddr::V6(address.ip),
        })
        .filter(is_lan_ip)
        .collect();
    addresses.sort();
    addresses.dedup();
    prefer_address(&mut addresses, default_route_ip());
    Ok(addresses)
}

/// Moves `preferred` to the front, inserting it when the interface scan missed
/// it. Invite links and the startup banner use the first address.
fn prefer_address(addresses: &mut Vec<IpAddr>, preferred: Option<IpAddr>) {
    let Some(preferred) = preferred else {
        return;
    };
    if let Some(index) = addresses.iter().position(|ip| *ip == preferred) {
        addresses.remove(index);
    }
    addresses.insert(0, preferred);
}

/// The source address of the default IPv4 route, if the host has one.
///
/// Connecting a UDP socket only selects a route and sends no packet.
fn default_route_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    is_lan_ip(&ip).then_some(ip)
}

fn is_lan_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !ip.is_loopback() && !ip.is_unspecified() && !ip.is_link_local(),
        IpAddr::V6(ip) => {
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_unicast_link_local()
                && !ip.is_multicast()
        }
    }
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
        let bind = cli.bind_address().expect("the default binds");
        assert_eq!(bind, "127.0.0.1:8787".parse().expect("a socket address"));
        assert!(bind.ip().is_loopback());
    }

    #[test]
    fn lan_alone_binds_every_interface() {
        // Arrange
        let cli = Cli::try_parse_from(["localtt", "serve", "--lan"]).expect("the CLI parses");

        // Act
        let bind = cli.bind_address().expect("--lan alone resolves");

        // Assert
        assert_eq!(bind, "0.0.0.0:8787".parse().expect("a socket address"));
    }

    #[test]
    fn a_non_loopback_bind_requires_lan() {
        // Arrange
        let cli = Cli::try_parse_from(["localtt", "serve", "--bind", "192.168.1.5:9000"])
            .expect("the CLI parses");

        // Act
        let error = cli
            .bind_address()
            .expect_err("a LAN bind without --lan is refused");

        // Assert
        assert!(error.to_string().contains("requires --lan"));
    }

    #[test]
    fn lan_refuses_an_explicit_loopback_bind() {
        // Arrange
        let cli = Cli::try_parse_from(["localtt", "serve", "--lan", "--bind", "127.0.0.1:9000"])
            .expect("the CLI parses");

        // Act
        let error = cli
            .bind_address()
            .expect_err("--lan with loopback is ambiguous");

        // Assert
        assert!(error.to_string().contains("non-loopback --bind"));
    }

    #[test]
    fn lan_accepts_a_non_loopback_bind() {
        // Arrange
        let cli = Cli::try_parse_from(["localtt", "serve", "--lan", "--bind", "192.168.1.5:9000"])
            .expect("the CLI parses");

        // Act
        let bind = cli.bind_address().expect("the LAN bind resolves");

        // Assert
        assert_eq!(bind, "192.168.1.5:9000".parse().expect("a socket address"));
    }

    #[test]
    fn lan_host_requires_lan() {
        let error = Cli::try_parse_from(["localtt", "serve", "--lan-host", "tt.local"])
            .expect_err("--lan-host alone is ambiguous");
        assert!(error.to_string().contains("--lan"));
    }

    #[test]
    fn lan_host_normalization_adds_the_default_port_and_keeps_explicit_ones() {
        assert_eq!(
            normalize_lan_host("tt.local", 8787).expect("a hostname normalizes"),
            "tt.local:8787"
        );
        assert_eq!(
            normalize_lan_host("tt.local:9000", 8787).expect("an explicit port is kept"),
            "tt.local:9000"
        );
        assert_eq!(
            normalize_lan_host("192.168.1.5", 8787).expect("an IPv4 host normalizes"),
            "192.168.1.5:8787"
        );
        assert_eq!(
            normalize_lan_host("[fd00::1]:9000", 8787).expect("a bracketed IPv6 normalizes"),
            "[fd00::1]:9000"
        );
        assert_eq!(
            normalize_lan_host("[fd00::1]", 8787).expect("a bracketed IPv6 gets the port"),
            "[fd00::1]:8787"
        );
        assert!(normalize_lan_host("http://tt.local", 8787).is_err());
        assert!(normalize_lan_host("tt.local/path", 8787).is_err());
        assert!(normalize_lan_host("tt.local:not-a-port", 8787).is_err());
        assert!(normalize_lan_host("  ", 8787).is_err());
    }

    #[test]
    fn lan_filter_keeps_routable_addresses_and_skips_local_only_ones() {
        for address in [
            "192.168.1.5",
            "10.0.0.2",
            "172.16.0.2",
            "fd00::1",
            "2001:db8::1",
        ] {
            let ip: IpAddr = address.parse().expect("an IP address");
            assert!(is_lan_ip(&ip), "{address} should be a LAN address");
        }
        for address in [
            "127.0.0.1",
            "0.0.0.0",
            "169.254.1.1",
            "::1",
            "::",
            "fe80::1",
        ] {
            let ip: IpAddr = address.parse().expect("an IP address");
            assert!(!is_lan_ip(&ip), "{address} should be skipped");
        }
    }

    #[test]
    fn the_default_route_address_is_offered_first() {
        // Arrange
        let mut addresses: Vec<IpAddr> = ["10.0.0.2", "172.17.0.1", "192.168.1.5"]
            .iter()
            .map(|raw| raw.parse().expect("an IP address"))
            .collect();
        let preferred: IpAddr = "192.168.1.5".parse().expect("an IP address");

        // Act
        prefer_address(&mut addresses, Some(preferred));

        // Assert
        assert_eq!(
            addresses,
            ["192.168.1.5", "10.0.0.2", "172.17.0.1"]
                .iter()
                .map(|raw| raw.parse().expect("an IP address"))
                .collect::<Vec<IpAddr>>()
        );
    }

    #[test]
    fn a_preferred_address_the_scan_missed_is_inserted_first() {
        // Arrange
        let mut addresses: Vec<IpAddr> = ["10.0.0.2", "192.168.1.5"]
            .iter()
            .map(|raw| raw.parse().expect("an IP address"))
            .collect();
        let preferred: IpAddr = "172.17.0.1".parse().expect("an IP address");

        // Act
        prefer_address(&mut addresses, Some(preferred));

        // Assert
        assert_eq!(
            addresses,
            ["172.17.0.1", "10.0.0.2", "192.168.1.5"]
                .iter()
                .map(|raw| raw.parse().expect("an IP address"))
                .collect::<Vec<IpAddr>>()
        );
    }

    #[test]
    fn player_urls_keep_invite_order_and_drop_duplicates() {
        // Arrange
        let lan_ips: Vec<IpAddr> = ["192.168.1.5", "172.17.0.1"]
            .iter()
            .map(|raw| raw.parse().expect("an IP address"))
            .collect();

        // Act
        let urls = player_urls(Some("192.168.1.5:8787"), &lan_ips, 8787);

        // Assert
        assert_eq!(urls, ["http://192.168.1.5:8787", "http://172.17.0.1:8787"]);
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
