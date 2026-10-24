//! Snapshot scenarios: resize, scroll, capture and report the browser UI.

use std::{fs, path::PathBuf, time::Duration};

use anyhow::Context;
use serde::Serialize;
use tokio::time::sleep;

use super::{artifact_root, browser::Browser};

const SETTLE: Duration = Duration::from_millis(250);

/// A named browser viewport used for a capture.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Viewport {
    name: &'static str,
    width: u32,
    height: u32,
}

impl Viewport {
    /// Builds a viewport with a stable name for artifact filenames.
    pub(crate) const fn new(name: &'static str, width: u32, height: u32) -> Self {
        Self {
            name,
            width,
            height,
        }
    }

    /// The viewport width in CSS pixels.
    #[must_use]
    pub(crate) const fn width(self) -> u32 {
        self.width
    }

    /// The viewport height in CSS pixels.
    #[must_use]
    pub(crate) const fn height(self) -> u32 {
        self.height
    }
}

/// A desktop-width viewport.
pub(crate) const DESKTOP: Viewport = Viewport::new("desktop", 1440, 900);

/// A narrow mobile-width viewport.
///
/// Headless Firefox enforces a minimum outer width, and window height includes
/// browser chrome, so the report records the actual CSS viewport next to this
/// request.
pub(crate) const MOBILE: Viewport = Viewport::new("mobile", 390, 930);

/// One screenshot and the semantic UI state read next to it.
#[derive(Debug, Serialize)]
struct Capture {
    index: usize,
    label: String,
    viewport: Viewport,
    actual: WindowSize,
    focus: Option<String>,
    screenshot: String,
    state: UiState,
}

/// The CSS-pixel viewport Firefox actually gave us.
#[derive(Debug, Serialize)]
struct WindowSize {
    width: u32,
    height: u32,
}

/// The user-facing state we care about when reviewing a screenshot.
#[derive(Debug, Default, Serialize)]
struct UiState {
    campaign: Option<String>,
    clock: Option<String>,
    stream: Option<String>,
    busy: Option<String>,
    error: Option<String>,
    entities: usize,
    selected: Option<String>,
    hp: Option<String>,
    temp_hp: Option<String>,
    ac: Option<String>,
    inventory: usize,
    feed: usize,
    reference_hits: usize,
}

impl UiState {
    async fn read(browser: &Browser) -> anyhow::Result<Self> {
        Ok(Self {
            campaign: trimmed(
                browser
                    .optional_text("[data-testid='campaign-name']")
                    .await?,
            ),
            clock: trimmed(browser.optional_text("[data-testid='clock']").await?),
            stream: trimmed(
                browser
                    .optional_text("[data-testid='stream-status']")
                    .await?,
            ),
            busy: trimmed(
                browser
                    .optional_attr("[data-testid='shell']", "data-busy")
                    .await?,
            ),
            error: trimmed(
                browser
                    .optional_attr("[data-testid='shell']", "data-error")
                    .await?,
            ),
            entities: browser.count("[data-testid='entity-row']").await?,
            selected: trimmed(browser.optional_text("[data-testid='entity-name']").await?),
            hp: trimmed(browser.optional_text("[data-testid='hp-value']").await?),
            temp_hp: trimmed(
                browser
                    .optional_text("[data-testid='temp-hp-value']")
                    .await?,
            ),
            ac: trimmed(browser.optional_text("[data-testid='ac-value']").await?),
            inventory: browser.count("[data-testid='inventory-row']").await?,
            feed: browser.count("[data-feed-id]").await?,
            reference_hits: browser.count("[data-testid='reference-hit']").await?,
        })
    }
}

/// Collects captures for one named UI scenario.
pub(crate) struct Scenario {
    name: String,
    output: PathBuf,
    captures: Vec<Capture>,
    error: Option<String>,
}

impl Scenario {
    /// Clears `target/ui/latest` and prepares a new scenario run.
    pub(crate) fn start(name: &str) -> anyhow::Result<Self> {
        let output = artifact_root().join("latest");
        if output.exists() {
            fs::remove_dir_all(&output).with_context(|| {
                format!("clearing the previous UI snapshot at {}", output.display())
            })?;
        }
        fs::create_dir_all(&output)
            .with_context(|| format!("creating the UI snapshot directory {}", output.display()))?;
        let output = fs::canonicalize(&output)
            .with_context(|| format!("resolving the UI snapshot directory {}", output.display()))?;
        Ok(Self {
            name: name.to_owned(),
            output,
            captures: Vec::new(),
            error: None,
        })
    }

    /// Captures the viewport from the top of the page.
    pub(crate) async fn capture(
        &mut self,
        browser: &Browser,
        label: &str,
        viewport: Viewport,
    ) -> anyhow::Result<()> {
        self.capture_with_focus(browser, label, viewport, None)
            .await
    }

    /// Captures the viewport after scrolling `focus` into view.
    pub(crate) async fn capture_scrolled(
        &mut self,
        browser: &Browser,
        label: &str,
        viewport: Viewport,
        focus: &str,
    ) -> anyhow::Result<()> {
        self.capture_with_focus(browser, label, viewport, Some(focus))
            .await
    }

    /// Records why the scenario stopped early; included in the report.
    pub(crate) fn record_error(&mut self, error: &anyhow::Error) {
        self.error = Some(format!("{error:#}"));
    }

    /// Writes `manifest.json` and `report.md` and returns the latest directory.
    pub(crate) fn finish(&self) -> anyhow::Result<PathBuf> {
        let manifest = serde_json::to_string_pretty(&Manifest {
            scenario: &self.name,
            error: self.error.as_deref(),
            captures: &self.captures,
        })
        .context("serializing the UI snapshot manifest")?;
        fs::write(self.output.join("manifest.json"), manifest)
            .context("writing the UI snapshot manifest")?;

        let report = self.report()?;
        fs::write(self.output.join("report.md"), report)
            .context("writing the UI snapshot report")?;
        Ok(self.output.clone())
    }

    async fn capture_with_focus(
        &mut self,
        browser: &Browser,
        label: &str,
        viewport: Viewport,
        focus: Option<&str>,
    ) -> anyhow::Result<()> {
        browser
            .set_window_size(viewport.width, viewport.height)
            .await?;
        browser.scroll_top().await?;
        if let Some(focus) = focus {
            browser.scroll_to(focus).await?;
        }
        sleep(SETTLE).await;

        let actual = browser.viewport_size().await?;
        let state = UiState::read(browser).await?;
        let pixels = browser.screenshot().await?;
        let index = self.captures.len() + 1;
        let screenshot = format!("{index:02}-{label}-{}.png", viewport.name);
        fs::write(self.output.join(&screenshot), pixels)
            .with_context(|| format!("writing the screenshot {screenshot}"))?;
        self.captures.push(Capture {
            index,
            label: label.to_owned(),
            viewport,
            actual: WindowSize {
                width: actual.0,
                height: actual.1,
            },
            focus: focus.map(str::to_owned),
            screenshot,
            state,
        });
        Ok(())
    }

    fn report(&self) -> anyhow::Result<String> {
        use std::fmt::Write as _;

        let mut report = String::new();
        writeln!(report, "# UI snapshot report")?;
        writeln!(report)?;
        writeln!(report, "Scenario: `{}`", self.name)?;
        if let Some(campaign) = self
            .captures
            .iter()
            .find_map(|capture| capture.state.campaign.as_deref())
        {
            writeln!(report, "Campaign: `{campaign}`")?;
        }
        writeln!(report, "Latest: `{}`", self.output.display())?;
        if let Some(error) = &self.error {
            writeln!(report)?;
            writeln!(report, "**Run error**")?;
            writeln!(report)?;
            writeln!(report, "```text")?;
            writeln!(report, "{error}")?;
            writeln!(report, "```")?;
        }

        for capture in &self.captures {
            writeln!(report)?;
            writeln!(
                report,
                "## {:02}. {} ({} requested {}x{}, actual {}x{})",
                capture.index,
                capture.label,
                capture.viewport.name,
                capture.viewport.width,
                capture.viewport.height,
                capture.actual.width,
                capture.actual.height
            )?;
            writeln!(report)?;
            writeln!(report, "![{}]({})", capture.label, capture.screenshot)?;
            writeln!(report)?;
            if let Some(focus) = &capture.focus {
                writeln!(report, "- focus: `{focus}`")?;
            }
            writeln!(
                report,
                "- campaign: {}",
                or_dash(capture.state.campaign.as_deref())
            )?;
            writeln!(
                report,
                "- clock: {}",
                or_dash(capture.state.clock.as_deref())
            )?;
            writeln!(
                report,
                "- stream: {}",
                or_dash(capture.state.stream.as_deref())
            )?;
            writeln!(report, "- busy: {}", or_dash(capture.state.busy.as_deref()))?;
            writeln!(
                report,
                "- error: {}",
                or_dash(capture.state.error.as_deref())
            )?;
            writeln!(report, "- entities: {}", capture.state.entities)?;
            writeln!(
                report,
                "- selected: {}",
                or_dash(capture.state.selected.as_deref())
            )?;
            writeln!(report, "- hp: {}", or_dash(capture.state.hp.as_deref()))?;
            writeln!(
                report,
                "- temp hp: {}",
                or_dash(capture.state.temp_hp.as_deref())
            )?;
            writeln!(report, "- ac: {}", or_dash(capture.state.ac.as_deref()))?;
            writeln!(report, "- inventory rows: {}", capture.state.inventory)?;
            writeln!(report, "- feed lines: {}", capture.state.feed)?;
            writeln!(report, "- reference hits: {}", capture.state.reference_hits)?;
        }
        Ok(report)
    }
}

#[derive(Serialize)]
struct Manifest<'a> {
    scenario: &'a str,
    error: Option<&'a str>,
    captures: &'a [Capture],
}

fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn or_dash(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}
