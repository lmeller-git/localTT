//! One server and browser session with failure capture and ordered teardown.

use anyhow::Context;

use super::{browser::Browser, server::TestServer};

/// A campaign, a browser session and the artifact name for failures.
pub(crate) struct Journey {
    pub(crate) server: TestServer,
    pub(crate) browser: Browser,
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
            server,
            browser,
            name,
        })
    }

    /// Saves a failure artifact, closes the browser and stops the server.
    ///
    /// Always consumes the journey; a failed body is captured and re-raised as
    /// a panic so the test name reports the failure.
    pub(crate) async fn finish(mut self, outcome: anyhow::Result<()>) {
        if let Err(error) = outcome {
            self.browser.capture_failure(self.name).await;
            let _ = self.browser.shutdown().await;
            let _ = self.server.shutdown().await;
            panic!("{} failed: {error:#}", self.name);
        }
        self.browser
            .shutdown()
            .await
            .expect("the browser session closes");
        self.server.shutdown().await.expect("the test server stops");
    }
}
