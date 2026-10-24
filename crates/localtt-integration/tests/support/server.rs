//! A real server on an ephemeral loopback port with a temporary campaign.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, ensure};
use localtt_adapter::{
    reference::FixtureSource,
    rng::ChaChaRng,
    store::{AsyncSqliteStore, SqliteStore},
};
use localtt_engine::content::ReferenceSource;
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
    app: AppHandle,
    server: ServerHandle,
    _temp: TempDir,
}

impl TestServer {
    /// Starts a fresh campaign and serves the built browser client from
    /// `web/dist`.
    pub(crate) async fn start(campaign_name: &str) -> anyhow::Result<Self> {
        let temp = TempDir::new().context("creating the browser-test data directory")?;
        let campaign_path = temp.path().join("campaign.sqlite");
        let mut store = SqliteStore::open(&campaign_path).context("opening the test store")?;
        let started = start(
            &mut store,
            Some(&CreateCampaign::new(campaign_name, "Test DM")),
            None,
        )
        .context("creating the browser-test campaign")?;

        let content: Arc<dyn ReferenceSource> = Arc::new(FixtureSource::new());
        let env = ServerEnv::with_rng(Arc::clone(&content), ChaChaRng::from_seed([7; 32]));
        let web_dir = web_dist()?;
        let app = AppHandle::start(
            AsyncSqliteStore::new(store),
            started.snapshot,
            Box::new(env),
            content,
        )
        .context("starting the browser-test actor")?;

        let config = ServerConfig::new(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            ServerToken::from_secret(TOKEN),
        )
        .with_web_dir(web_dir);
        let server = match serve(app.clone(), started.actor, config).await {
            Ok(server) => server,
            Err(error) => {
                let _ = app.shutdown().await;
                return Err(error).context("starting the browser-test HTTP server");
            }
        };
        let base_url = format!("http://{}", server.local_addr());

        Ok(Self {
            base_url,
            app,
            server,
            _temp: temp,
        })
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

fn web_dist() -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    ensure!(
        path.join("index.html").is_file(),
        "web assets are missing at {}; run `just web-build` first",
        path.display()
    );
    Ok(path)
}
