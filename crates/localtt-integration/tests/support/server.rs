//! A real server on an ephemeral loopback port with a temporary campaign.

use std::{
    net::SocketAddr,
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
    http::{ServerConfig, ServerHandle, ServerToken, serve},
    session::{CreateCampaign, start},
};
use tempfile::TempDir;

/// The token the test browser enters on the login form.
pub(crate) const TOKEN: &str = "browser-e2e-token";

/// A running HTTP/WebSocket server and its temporary campaign.
pub(crate) struct TestServer {
    base_url: String,
    addr: SocketAddr,
    app: AppHandle,
    server: ServerHandle,
    temp: Option<TempDir>,
    campaign_path: PathBuf,
}

impl TestServer {
    /// Starts a fresh campaign and serves the built browser client from
    /// `web/dist`.
    pub(crate) async fn start(campaign_name: &str) -> anyhow::Result<Self> {
        let temp = TempDir::new().context("creating the browser-test data directory")?;
        let campaign_path = temp.path().join("campaign.sqlite");
        boot(
            temp,
            campaign_path,
            Some(CreateCampaign::new(campaign_name, "Test DM")),
            &[],
            SocketAddr::from(([127, 0, 0, 1], 0)),
        )
        .await
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
    pub(crate) async fn restart(mut self, commands: &[Command]) -> anyhow::Result<Self> {
        let addr = self.addr;
        let temp = self
            .temp
            .take()
            .context("the test data directory is still owned")?;
        let campaign_path = self.campaign_path.clone();
        self.server.stop();
        self.app
            .shutdown()
            .await
            .context("stopping the browser-test actor")?;
        boot(temp, campaign_path, None, commands, addr)
            .await
            .context("restarting the browser-test server")
    }

    /// The origin the browser should open.
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The bearer token the browser should use.
    pub(crate) fn token(&self) -> &str {
        TOKEN
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

/// Opens the campaign store, starts the actor and serves the browser assets.
async fn boot(
    temp: TempDir,
    campaign_path: PathBuf,
    create: Option<CreateCampaign>,
    commands: &[Command],
    addr: SocketAddr,
) -> anyhow::Result<TestServer> {
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
    for command in commands {
        app.execute(started.actor, command.clone())
            .await
            .context("applying a pre-serve test command")?;
    }

    let config = ServerConfig::new(addr, ServerToken::from_secret(TOKEN))
        .with_web_dir(web_dist()?)
        .with_diagnostics(true);
    let server = match serve(app.clone(), started.actor, config).await {
        Ok(server) => server,
        Err(error) => {
            let _ = app.shutdown().await;
            return Err(error).context("starting the browser-test HTTP server");
        }
    };
    let addr = server.local_addr();

    Ok(TestServer {
        base_url: format!("http://{addr}"),
        addr,
        app,
        server,
        temp: Some(temp),
        campaign_path,
    })
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
