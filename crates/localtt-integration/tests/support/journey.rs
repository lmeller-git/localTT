//! One server and browser session with failure capture and ordered teardown.

use anyhow::Context;
use localtt_engine::commands::Command;

use super::{
    browser::Browser,
    server::{StoppedServer, TestServer},
};

/// A campaign, a browser session and the artifact name for failures.
pub(crate) struct Journey {
    server: Option<TestServer>,
    stopped: Option<StoppedServer>,
    browser: Browser,
    name: &'static str,
}

impl Journey {
    /// Starts a fresh campaign and opens it in headless Firefox.
    ///
    /// The server is stopped when the browser cannot start, so a failed launch
    /// never leaks a listener or a temporary directory.
    pub(crate) async fn start(name: &'static str, campaign: &str) -> anyhow::Result<Self> {
        let mut server = TestServer::start(campaign)
            .await
            .with_context(|| format!("starting the {campaign} server"))?;
        let browser = match Browser::launch(server.base_url()).await {
            Ok(browser) => browser,
            Err(error) => {
                let _ = server.shutdown().await;
                return Err(error).with_context(|| format!("opening {campaign} in the browser"));
            }
        };
        Ok(Self {
            server: Some(server),
            stopped: None,
            browser,
            name,
        })
    }

    /// The running server.
    ///
    /// # Panics
    ///
    /// Only when called while the server is stopped.
    pub(crate) fn server(&self) -> &TestServer {
        self.server
            .as_ref()
            .expect("the journey started its server before reading it")
    }

    /// The browser session.
    pub(crate) fn browser(&self) -> &Browser {
        &self.browser
    }

    /// Stops the server and keeps its port, token and data.
    pub(crate) async fn stop(&mut self) -> anyhow::Result<()> {
        let server = self
            .server
            .take()
            .context("the journey still owns its server")?;
        self.stopped = Some(server.stop().await?);
        Ok(())
    }

    /// Starts the stopped server again, applying `commands` before the listener
    /// binds so the browser can only learn them by resyncing.
    pub(crate) async fn resume(&mut self, commands: &[Command]) -> anyhow::Result<()> {
        let stopped = self
            .stopped
            .take()
            .context("the journey stopped its server before resuming it")?;
        self.server = Some(stopped.start(commands).await?);
        Ok(())
    }

    /// Restarts the server on the same port and data, applying `commands`
    /// before the listener binds again.
    pub(crate) async fn restart(&mut self, commands: &[Command]) -> anyhow::Result<()> {
        self.stop().await?;
        self.resume(commands).await
    }

    /// Restarts the server with a different bearer token, so the browser's
    /// next authenticated call is rejected.
    pub(crate) async fn restart_with_token(
        &mut self,
        commands: &[Command],
        token: &str,
    ) -> anyhow::Result<()> {
        self.stop().await?;
        let stopped = self
            .stopped
            .take()
            .context("the journey stopped its server before restarting it")?;
        self.server = Some(stopped.start_with_token(commands, token).await?);
        Ok(())
    }

    /// Saves a failure artifact, closes the browser and stops the server.
    ///
    /// Always consumes the journey; a failed body is captured and re-raised as
    /// a panic so the test name reports the failure.
    pub(crate) async fn finish(mut self, outcome: anyhow::Result<()>) {
        if let Err(error) = outcome {
            self.browser.capture_failure(self.name).await;
            let _ = self.browser.shutdown().await;
            if let Some(mut server) = self.server.take() {
                let _ = server.shutdown().await;
            }
            panic!("{} failed: {error:#}", self.name);
        }
        self.browser
            .shutdown()
            .await
            .expect("the browser session closes");
        if let Some(mut server) = self.server.take() {
            server.shutdown().await.expect("the test server stops");
        }
    }
}
