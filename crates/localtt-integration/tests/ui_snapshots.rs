//! UI snapshot loop: drive a representative journey, then capture and report it.
//!
//! The test is ignored by the normal gate because it needs Firefox and
//! geckodriver. Run it with `just ui-shot` and inspect `target/ui/latest`.

mod support;

use anyhow::{Context, ensure};
use support::{
    browser::Browser,
    scenario::{DESKTOP, MOBILE, Scenario},
    server::TestServer,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just ui-shot`"]
async fn ui_snapshots() {
    let mut server = TestServer::start("UI Snapshot")
        .await
        .expect("the browser-test server starts");
    let mut browser = match Browser::launch(server.base_url()).await {
        Ok(browser) => browser,
        Err(error) => {
            let _ = server.shutdown().await;
            panic!("the browser starts: {error:#}");
        }
    };
    let mut scenario = match Scenario::start("character-sheet") {
        Ok(scenario) => scenario,
        Err(error) => {
            let _ = browser.shutdown().await;
            let _ = server.shutdown().await;
            panic!("the UI scenario starts: {error:#}");
        }
    };

    let result = run_scenario(&server, &browser, &mut scenario).await;
    if let Err(error) = &result {
        scenario.record_error(error);
        browser.capture_failure("ui-snapshot").await;
    }
    let report = scenario.finish();
    browser
        .shutdown()
        .await
        .expect("the browser session closes");
    server.shutdown().await.expect("the test server stops");

    if let Err(error) = result {
        panic!("UI snapshot scenario failed: {error:#}");
    }
    let report = report.expect("the UI snapshot report is written");
    println!("UI snapshots written to {}", report.display());
}

async fn run_scenario(
    server: &TestServer,
    browser: &Browser,
    scenario: &mut Scenario,
) -> anyhow::Result<()> {
    browser
        .wait_for("[data-testid='token-input']")
        .await
        .context("the login form mounts")?;
    scenario.capture(browser, "login", DESKTOP).await?;
    scenario.capture(browser, "login", MOBILE).await?;

    browser
        .set_window_size(DESKTOP.width(), DESKTOP.height())
        .await?;
    browser.login(server.token()).await?;
    wait_ready(browser).await?;

    let campaign = browser.text("[data-testid='campaign-name']").await?;
    ensure!(
        campaign.trim() == "UI Snapshot",
        "unexpected campaign name {campaign:?}"
    );

    create_character(browser).await?;
    add_item(browser, "Spellbook", "1", "equipped").await?;
    add_item(browser, "Potion of Healing", "2", "carried").await?;
    roll(browser).await?;
    advance_clock(browser).await?;

    scenario.capture(browser, "character", DESKTOP).await?;
    scenario.capture(browser, "character", MOBILE).await?;
    scenario
        .capture_scrolled(
            browser,
            "character-detail",
            MOBILE,
            "[data-testid='detail-panel']",
        )
        .await?;

    browser
        .set_window_size(DESKTOP.width(), DESKTOP.height())
        .await?;
    browser.scroll_top().await?;
    browser
        .click("[data-testid='debug-button']")
        .await
        .context("the debug panel opens")?;
    browser
        .wait_for("[data-testid='diagnostics']")
        .await
        .context("the diagnostics endpoint answers")?;
    scenario.capture(browser, "debug", DESKTOP).await?;
    Ok(())
}

async fn wait_ready(browser: &Browser) -> anyhow::Result<()> {
    browser
        .wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the campaign becomes ready")?;
    browser
        .wait_attr("[data-testid='shell']", "data-error", "false")
        .await
        .context("the initial reads do not report an error")?;
    browser
        .wait_attr("[data-testid='shell']", "data-ws-state", "live")
        .await
        .context("the event stream connects")?;
    wait_idle(browser).await
}

async fn wait_idle(browser: &Browser) -> anyhow::Result<()> {
    browser
        .wait_attr("[data-testid='shell']", "data-busy", "false")
        .await
        .context("the current command finishes")?;
    Ok(())
}

async fn create_character(browser: &Browser) -> anyhow::Result<()> {
    browser
        .click("[data-testid='character-create'] summary")
        .await?;
    browser
        .fill("[data-testid='character-name']", "Aria Vale")
        .await?;
    browser
        .fill("[data-testid='character-class']", "Wizard")
        .await?;
    browser.fill("[data-testid='character-level']", "5").await?;
    browser
        .fill("[data-testid='character-max-hp']", "27")
        .await?;
    browser
        .fill("[data-testid='character-abilities']", "8,14,14,18,12,10")
        .await?;
    browser
        .click("[data-testid='create-character-button']")
        .await?;
    wait_idle(browser).await?;
    browser
        .wait_text("[data-testid='entity-name']", "Aria Vale")
        .await
        .context("the created character is selected")?;
    browser
        .wait_text("[data-testid='hp-value']", "27/27")
        .await
        .context("the character sheet shows the created HP")?;
    browser
        .wait_attr("[data-testid='entity-detail']", "data-writable", "true")
        .await
        .context("the produced character is writable")?;
    Ok(())
}

async fn add_item(
    browser: &Browser,
    item_name: &str,
    quantity: &str,
    state: &str,
) -> anyhow::Result<()> {
    browser
        .fill("[data-testid='inventory-item-name']", item_name)
        .await?;
    browser
        .fill("[data-testid='inventory-item-qty']", quantity)
        .await?;
    browser
        .select_value("[data-testid='inventory-item-state']", state)
        .await?;
    browser
        .click("[data-testid='inventory-add-button']")
        .await?;
    wait_idle(browser).await?;
    browser
        .wait_text("[data-testid='inventory-section']", item_name)
        .await
        .with_context(|| format!("the inventory shows {item_name}"))?;
    Ok(())
}

async fn roll(browser: &Browser) -> anyhow::Result<()> {
    browser
        .fill("[data-testid='roll-expression']", "1d20+5")
        .await?;
    browser.click("[data-testid='roll-button']").await?;
    wait_idle(browser).await?;
    browser
        .wait_text("[data-testid='feed-panel']", "rolled")
        .await
        .context("the dice result reaches the feed")?;
    Ok(())
}

async fn advance_clock(browser: &Browser) -> anyhow::Result<()> {
    browser.fill("[data-testid='clock-amount']", "1").await?;
    browser
        .select_value("[data-testid='clock-unit']", "hours")
        .await?;
    browser
        .click("[data-testid='clock-advance-button']")
        .await?;
    wait_idle(browser).await?;
    browser
        .wait_text("[data-testid='clock']", "01:00:00")
        .await
        .context("the clock advances one hour")?;
    Ok(())
}
