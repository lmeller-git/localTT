//! UI snapshot loop: drive a representative journey, then capture and report it.
//!
//! The test is ignored by the normal gate because it needs Firefox and
//! geckodriver. Run it with `just ui-shot` and inspect `target/ui/latest`.

mod support;

use anyhow::{Context, ensure};
use support::{
    browser::Browser,
    flows::{self, Character},
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
    browser.enter_campaign(server.token()).await?;

    let campaign = browser.text("[data-testid='campaign-name']").await?;
    ensure!(
        campaign.trim() == "UI Snapshot",
        "unexpected campaign name {campaign:?}"
    );

    flows::create_character(
        browser,
        &Character::new("Aria Vale")
            .class("Wizard")
            .level(5)
            .max_hp(27)
            .abilities("8,14,14,18,12,10"),
    )
    .await?;
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
    browser.wait_idle().await?;
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
    browser.wait_idle().await?;
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
    browser.wait_idle().await?;
    browser
        .wait_text("[data-testid='clock']", "01:00:00")
        .await
        .context("the clock advances one hour")?;
    Ok(())
}
