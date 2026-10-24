//! Headless Firefox driven through geckodriver.

use std::{
    fs,
    net::{SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, bail, ensure};
use fantoccini::{
    Client,
    ClientBuilder,
    Locator,
    elements::Element,
    error::{CmdError, ErrorStatus},
    wd::Capabilities,
};
use serde_json::json;
use tokio::time::sleep;

use super::artifact_root;

const ELEMENT_TIMEOUT: Duration = Duration::from_secs(15);
const WAIT_PERIOD: Duration = Duration::from_millis(100);

/// A browser session and the geckodriver process that owns it.
pub(crate) struct Browser {
    client: Option<Client>,
    driver: Option<Child>,
}

impl Browser {
    /// Starts geckodriver and opens `base_url` in headless Firefox.
    pub(crate) async fn launch(base_url: &str) -> anyhow::Result<Self> {
        let port = free_port().context("reserving a geckodriver port")?;
        let mut driver = spawn_driver(port).context("starting geckodriver")?;
        if let Err(error) = wait_for_driver(port, &mut driver).await {
            let _ = driver.kill();
            let _ = driver.wait();
            return Err(error);
        }

        let mut capabilities = Capabilities::new();
        capabilities.insert("browserName".to_owned(), json!("firefox"));
        capabilities.insert(
            "moz:firefoxOptions".to_owned(),
            json!({"args": ["-headless"]}),
        );

        let mut builder = ClientBuilder::native();
        builder.capabilities(capabilities);
        let client = match builder.connect(&format!("http://127.0.0.1:{port}")).await {
            Ok(client) => client,
            Err(error) => {
                let _ = driver.kill();
                let _ = driver.wait();
                return Err(error).context("opening the Firefox WebDriver session");
            }
        };
        if let Err(error) = start_browser(&client, base_url).await {
            let _ = client.close().await;
            let _ = driver.kill();
            let _ = driver.wait();
            return Err(error);
        }

        Ok(Self {
            client: Some(client),
            driver: Some(driver),
        })
    }

    /// Enters the token on the login form and connects.
    pub(crate) async fn login(&self, token: &str) -> anyhow::Result<()> {
        let input = self.wait_element("[data-testid='token-input']").await?;
        input.send_keys(token).await?;
        self.click("[data-testid='connect-button']").await?;
        Ok(())
    }

    /// Waits until an element has a specific attribute value.
    pub(crate) async fn wait_attr(
        &self,
        css: &str,
        attribute: &str,
        expected: &str,
    ) -> anyhow::Result<Element> {
        let deadline = Instant::now() + ELEMENT_TIMEOUT;
        loop {
            match self.client().find(Locator::Css(css)).await {
                Ok(element) => {
                    if element.attr(attribute).await?.as_deref() == Some(expected) {
                        return Ok(element);
                    }
                }
                Err(CmdError::Standard(error)) if error.error == ErrorStatus::NoSuchElement => {}
                Err(error) => return Err(error.into()),
            }
            ensure!(
                Instant::now() < deadline,
                "timed out waiting for {css} to have {attribute}={expected}"
            );
            sleep(WAIT_PERIOD).await;
        }
    }

    /// Waits until an element's text contains `expected`.
    pub(crate) async fn wait_text(&self, css: &str, expected: &str) -> anyhow::Result<Element> {
        let deadline = Instant::now() + ELEMENT_TIMEOUT;
        loop {
            let elements = match self.find_all(css).await {
                Ok(elements) => elements,
                Err(CmdError::Standard(error)) if error.error == ErrorStatus::NoSuchElement => {
                    Vec::new()
                }
                Err(error) => return Err(error.into()),
            };
            for element in elements {
                if element.text().await?.contains(expected) {
                    return Ok(element);
                }
            }
            ensure!(
                Instant::now() < deadline,
                "timed out waiting for {css} to contain {expected:?}"
            );
            sleep(WAIT_PERIOD).await;
        }
    }

    /// Clears a field and types `value`.
    pub(crate) async fn fill(&self, css: &str, value: &str) -> anyhow::Result<()> {
        let element = self.wait_element(css).await?;
        element.clear().await?;
        element.send_keys(value).await?;
        Ok(())
    }

    /// Clicks the first element matching `css`.
    pub(crate) async fn click(&self, css: &str) -> anyhow::Result<()> {
        self.wait_element(css).await?.click().await?;
        Ok(())
    }

    /// Reads an element's text.
    pub(crate) async fn text(&self, css: &str) -> anyhow::Result<String> {
        Ok(self.wait_element(css).await?.text().await?)
    }

    /// Waits for an element to exist.
    pub(crate) async fn wait_for(&self, css: &str) -> anyhow::Result<()> {
        self.wait_element(css).await?;
        Ok(())
    }

    /// Reads an element's text when it is present.
    pub(crate) async fn optional_text(&self, css: &str) -> anyhow::Result<Option<String>> {
        match self.client().find(Locator::Css(css)).await {
            Ok(element) => Ok(Some(element.text().await?)),
            Err(CmdError::Standard(error)) if error.error == ErrorStatus::NoSuchElement => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads an element attribute when the element is present.
    pub(crate) async fn optional_attr(
        &self,
        css: &str,
        attribute: &str,
    ) -> anyhow::Result<Option<String>> {
        match self.client().find(Locator::Css(css)).await {
            Ok(element) => Ok(element.attr(attribute).await?),
            Err(CmdError::Standard(error)) if error.error == ErrorStatus::NoSuchElement => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Counts elements matching `css`.
    pub(crate) async fn count(&self, css: &str) -> anyhow::Result<usize> {
        Ok(self.find_all(css).await?.len())
    }

    /// Picks a select option by value.
    pub(crate) async fn select_value(&self, css: &str, value: &str) -> anyhow::Result<()> {
        self.wait_element(css).await?.select_by_value(value).await?;
        Ok(())
    }

    /// Changes the browser window size.
    pub(crate) async fn set_window_size(&self, width: u32, height: u32) -> anyhow::Result<()> {
        self.client()
            .set_window_size(width, height)
            .await
            .context("resizing the browser window")
    }

    /// Captures the visible viewport as PNG.
    pub(crate) async fn screenshot(&self) -> anyhow::Result<Vec<u8>> {
        self.client()
            .screenshot()
            .await
            .context("capturing the browser screenshot")
    }

    /// Reads the CSS-pixel viewport size after browser chrome and window limits.
    pub(crate) async fn viewport_size(&self) -> anyhow::Result<(u32, u32)> {
        let value = self
            .client()
            .execute(
                "return [window.innerWidth, window.innerHeight];",
                Vec::new(),
            )
            .await
            .context("reading the browser viewport size")?;
        let width = value
            .get(0)
            .and_then(serde_json::Value::as_u64)
            .context("the browser reports a numeric viewport width")?;
        let height = value
            .get(1)
            .and_then(serde_json::Value::as_u64)
            .context("the browser reports a numeric viewport height")?;
        Ok((
            u32::try_from(width).context("the viewport width fits in u32")?,
            u32::try_from(height).context("the viewport height fits in u32")?,
        ))
    }

    /// Scrolls the page to the top.
    pub(crate) async fn scroll_top(&self) -> anyhow::Result<()> {
        self.client()
            .execute("window.scrollTo(0, 0);", Vec::new())
            .await
            .context("scrolling the page to the top")?;
        Ok(())
    }

    /// Scrolls an element into view.
    pub(crate) async fn scroll_to(&self, css: &str) -> anyhow::Result<()> {
        let element = self.wait_element(css).await?;
        let argument =
            serde_json::to_value(element).context("serializing the scroll target element")?;
        self.client()
            .execute(
                "arguments[0].scrollIntoView({block: 'start'});",
                vec![argument],
            )
            .await
            .context("scrolling the target element into view")?;
        Ok(())
    }

    /// Saves a screenshot and page source for a failed test when possible.
    pub(crate) async fn capture_failure(&self, name: &str) {
        let directory = artifact_root().join("failure");
        if fs::create_dir_all(&directory).is_err() {
            return;
        }
        let Some(client) = self.client.as_ref() else {
            return;
        };
        if let Ok(pixels) = client.screenshot().await {
            let _ = fs::write(directory.join(format!("{name}.png")), pixels);
        }
        if let Ok(source) = client.source().await {
            let _ = fs::write(directory.join(format!("{name}.html")), source);
        }
        if let Ok(url) = client.current_url().await {
            let _ = fs::write(directory.join(format!("{name}.url")), url.as_str());
        }
    }

    /// Closes the WebDriver session and stops geckodriver.
    pub(crate) async fn shutdown(&mut self) -> anyhow::Result<()> {
        let result = if let Some(client) = self.client.take() {
            client.close().await.context("closing the browser session")
        } else {
            Ok(())
        };
        self.stop_driver();
        result
    }

    async fn wait_element(&self, css: &str) -> anyhow::Result<Element> {
        self.client()
            .wait()
            .at_most(ELEMENT_TIMEOUT)
            .every(WAIT_PERIOD)
            .for_element(Locator::Css(css))
            .await
            .with_context(|| format!("waiting for {css}"))
    }

    async fn find_all(&self, css: &str) -> Result<Vec<Element>, CmdError> {
        self.client().find_all(Locator::Css(css)).await
    }

    fn client(&self) -> &Client {
        self.client
            .as_ref()
            .expect("browser methods are not called after shutdown")
    }

    fn stop_driver(&mut self) {
        if let Some(mut driver) = self.driver.take() {
            let _ = driver.kill();
            let _ = driver.wait();
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.stop_driver();
    }
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn spawn_driver(port: u16) -> std::io::Result<Child> {
    let binary = std::env::var("GECKODRIVER").unwrap_or_else(|_| "geckodriver".to_owned());
    Command::new(binary)
        .arg("--port")
        .arg(port.to_string())
        .arg("--log")
        .arg("error")
        .env("MOZ_HEADLESS", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

async fn start_browser(client: &Client, base_url: &str) -> anyhow::Result<()> {
    client
        .set_window_size(1440, 900)
        .await
        .context("sizing the browser window")?;
    client
        .goto(base_url)
        .await
        .context("opening the browser client")?;
    Ok(())
}

async fn wait_for_driver(port: u16, driver: &mut Child) -> anyhow::Result<()> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        if let Some(status) = driver.try_wait()? {
            bail!("geckodriver exited before accepting connections: {status}");
        }
        if Instant::now() >= deadline {
            let _ = driver.kill();
            bail!("geckodriver did not accept connections on {address}");
        }
        sleep(WAIT_PERIOD).await;
    }
}
