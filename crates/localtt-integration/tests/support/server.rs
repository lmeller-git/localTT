//! A real server on an ephemeral port with a temporary campaign.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, ensure};
use localtt_adapter::{
    reference::FixtureSource,
    rng::ChaChaRng,
    store::{AsyncSqliteStore, SqliteStore},
};
use localtt_engine::{commands::Command, content::ReferenceSource};
use localtt_server::{
    app::AppHandle,
    env::ServerEnv,
    http::{ServerConfig, ServerHandle, serve},
    identity::hash_token,
    session::{CreateCampaign, start},
};
use tempfile::TempDir;

/// The token the test browser enters on the login form.
pub(crate) const TOKEN: &str = "browser-e2e-token";

/// A running HTTP/WebSocket server and its temporary campaign.
pub(crate) struct TestServer {
    base_url: String,
    addr: SocketAddr,
    token: String,
    app: AppHandle,
    server: ServerHandle,
    temp: Option<TempDir>,
    campaign_path: PathBuf,
    lan_ips: Vec<IpAddr>,
    advertise: Option<IpAddr>,
}

impl TestServer {
    /// Starts a fresh campaign and serves the built browser client from
    /// `web/dist`.
    pub(crate) async fn start(campaign_name: &str) -> anyhow::Result<Self> {
        Self::start_on(
            campaign_name,
            SocketAddr::from(([127, 0, 0, 1], 0)),
            Vec::new(),
            None,
        )
        .await
    }

    /// Starts the server on every interface so a browser can open it through a
    /// non-loopback address, which is not a secure context.
    pub(crate) async fn start_lan(campaign_name: &str) -> anyhow::Result<Self> {
        let ip =
            lan_ipv4().context("the LAN journey needs a non-loopback IPv4 address on this host")?;
        Self::start_on(
            campaign_name,
            SocketAddr::from(([0, 0, 0, 0], 0)),
            vec![IpAddr::V4(ip)],
            Some(IpAddr::V4(ip)),
        )
        .await
    }

    async fn start_on(
        campaign_name: &str,
        bind: SocketAddr,
        lan_ips: Vec<IpAddr>,
        advertise: Option<IpAddr>,
    ) -> anyhow::Result<Self> {
        let temp = TempDir::new().context("creating the browser-test data directory")?;
        let campaign_path = temp.path().join("campaign.sqlite");
        let config = BootConfig {
            create: Some(CreateCampaign::new(campaign_name, "Test DM")),
            addr: bind,
            token: TOKEN.to_owned(),
            lan_ips,
            advertise,
        };
        boot(temp, campaign_path, config, &[]).await
    }

    /// Stops the server and starts it again on the same port and data.
    ///
    /// `commands` are applied through the actor after it loads the campaign and
    /// before the listener binds, so a reconnecting client cannot have received
    /// them as events. The browser must resync over REST to see them.
    ///
    /// # Errors
    ///
    /// Propagates store, actor and bind failures, including a port that another
    /// process took while the server was down.
    pub(crate) async fn restart(self, commands: &[Command]) -> anyhow::Result<Self> {
        self.stop().await?.start(commands).await
    }

    /// Stops the server and starts it again with a different bearer token.
    ///
    /// The browser keeps the old credential, so its next authenticated call is
    /// rejected.
    pub(crate) async fn restart_with_token(
        self,
        commands: &[Command],
        token: &str,
    ) -> anyhow::Result<Self> {
        self.stop().await?.start_with_token(commands, token).await
    }

    /// Stops the listener and joins the actor thread, keeping the data.
    pub(crate) async fn stop(mut self) -> anyhow::Result<StoppedServer> {
        let temp = self
            .temp
            .take()
            .context("the test data directory is still owned")?;
        self.server.stop();
        self.app
            .shutdown()
            .await
            .context("stopping the browser-test actor")?;
        Ok(StoppedServer {
            addr: self.addr,
            token: self.token.clone(),
            temp,
            campaign_path: self.campaign_path.clone(),
            lan_ips: self.lan_ips.clone(),
            advertise: self.advertise,
        })
    }

    /// The origin the browser should open.
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The bearer token the browser should use.
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// Stops the listener and joins the actor thread.
    pub(crate) async fn shutdown(&mut self) -> anyhow::Result<()> {
        self.server.stop();
        self.app
            .shutdown()
            .await
            .context("stopping the browser-test actor")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.server.stop();
    }
}

/// A stopped server's address, token and data, ready to be started again.
pub(crate) struct StoppedServer {
    addr: SocketAddr,
    token: String,
    temp: TempDir,
    campaign_path: PathBuf,
    lan_ips: Vec<IpAddr>,
    advertise: Option<IpAddr>,
}

impl StoppedServer {
    /// Starts the server again with the same token.
    pub(crate) async fn start(self, commands: &[Command]) -> anyhow::Result<TestServer> {
        let token = self.token.clone();
        self.start_with_token(commands, &token).await
    }

    /// Starts the server again with a different bearer token.
    pub(crate) async fn start_with_token(
        self,
        commands: &[Command],
        token: &str,
    ) -> anyhow::Result<TestServer> {
        boot(
            self.temp,
            self.campaign_path,
            BootConfig {
                create: None,
                addr: self.addr,
                token: token.to_owned(),
                lan_ips: self.lan_ips,
                advertise: self.advertise,
            },
            commands,
        )
        .await
        .context("restarting the browser-test server")
    }
}

/// How a browser-test server is created or reopened.
struct BootConfig {
    create: Option<CreateCampaign>,
    addr: SocketAddr,
    token: String,
    lan_ips: Vec<IpAddr>,
    advertise: Option<IpAddr>,
}

/// Opens the campaign store, starts the actor and serves the browser assets.
async fn boot(
    temp: TempDir,
    campaign_path: PathBuf,
    config: BootConfig,
    commands: &[Command],
) -> anyhow::Result<TestServer> {
    let BootConfig {
        create,
        addr,
        token,
        lan_ips,
        advertise,
    } = config;
    let mut store = open_store(&campaign_path)?;
    let started = start(&mut store, create.as_ref(), None)
        .context("creating or loading the browser-test campaign")?;

    let content: Arc<dyn ReferenceSource> = Arc::new(FixtureSource::new());
    let env = ServerEnv::with_rng(Arc::clone(&content), ChaChaRng::from_seed([7; 32]));
    let app = AppHandle::start(
        AsyncSqliteStore::new(store),
        started.snapshot,
        Box::new(env),
        content,
    )
    .context("starting the browser-test actor")?;
    app.execute(
        started.actor,
        Command::RotateDmKey {
            player: started.actor,
            token_hash: hash_token(&token),
        },
    )
    .await
    .context("setting the browser-test DM key")?;
    for command in commands {
        app.execute(started.actor, command.clone())
            .await
            .context("applying a pre-serve test command")?;
    }

    let mut server_config = ServerConfig::new(addr)
        .with_web_dir(web_dist()?)
        .with_diagnostics(true);
    if !lan_ips.is_empty() {
        server_config = server_config.with_lan(None, lan_ips.iter().copied());
    }
    let server = match serve(app.clone(), server_config).await {
        Ok(server) => server,
        Err(error) => {
            let _ = app.shutdown().await;
            return Err(error).context("starting the browser-test HTTP server");
        }
    };
    let addr = server.local_addr();
    let advertised = SocketAddr::new(advertise.unwrap_or_else(|| addr.ip()), addr.port());

    Ok(TestServer {
        base_url: format!("http://{advertised}"),
        addr,
        token,
        app,
        server,
        temp: Some(temp),
        campaign_path,
        lan_ips,
        advertise,
    })
}

/// The host's default-route IPv4 address, without sending any packets.
fn lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

fn open_store(campaign_path: &Path) -> anyhow::Result<SqliteStore> {
    SqliteStore::open(campaign_path)
        .with_context(|| format!("opening the test store at {}", campaign_path.display()))
}

fn web_dist() -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    ensure!(
        path.join("index.html").is_file(),
        "web assets are missing at {}; run `just web-build` first",
        path.display()
    );
    Ok(path)
}
