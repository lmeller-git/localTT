//! Browser journeys through the real HTTP/WS server and Trunk assets.
//!
//! These tests need Firefox and geckodriver, so they are ignored by the normal
//! `just test` gate. Run them with `just e2e`.

mod support;

use anyhow::{Context, ensure};
use support::{
    browser::Browser,
    flows::{self, Character},
    journey::Journey,
    server::TestServer,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_login_create_and_save() {
    let journey = Journey::start("browser-smoke", "Browser E2E")
        .await
        .expect("the browser-test server and browser start");
    let outcome = session_smoke(&journey.server, &journey.browser).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_hp_temp_and_long_rest() {
    let journey = Journey::start("browser-hp-rest", "Browser HP")
        .await
        .expect("the browser-test server and browser start");
    let outcome = hp_temp_and_rest(&journey.server, &journey.browser).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_handout_give() {
    let journey = Journey::start("browser-handout", "Browser Handout")
        .await
        .expect("the browser-test server and browser start");
    let outcome = handout_give(&journey.server, &journey.browser).await;
    journey.finish(outcome).await;
}

async fn session_smoke(server: &TestServer, browser: &Browser) -> anyhow::Result<()> {
    browser.enter_campaign(server.token()).await?;

    let campaign = browser.text("[data-testid='campaign-name']").await?;
    ensure!(
        campaign.trim() == "Browser E2E",
        "unexpected campaign name {campaign:?}"
    );
    let entities = browser.text("[data-testid='entities-panel']").await?;
    ensure!(
        entities.contains("No visible entities."),
        "a fresh campaign should have no entities: {entities:?}"
    );

    flows::create_character(
        browser,
        &Character::new("Aria Test")
            .class("Wizard")
            .level(3)
            .max_hp(21)
            .abilities("8,14,14,18,12,10"),
    )
    .await?;

    browser
        .click("[data-testid='save-button']")
        .await
        .context("the save button is available")?;
    browser.wait_idle().await?;
    browser
        .wait_text("[data-testid='status']", "saved")
        .await
        .context("the save status is visible")?;

    browser
        .click("[data-testid='debug-button']")
        .await
        .context("the debug panel opens")?;
    browser
        .wait_for("[data-testid='diagnostics']")
        .await
        .context("the diagnostics endpoint answers")?;
    browser
        .wait_text("[data-testid='debug-ring']", "campaign saved")
        .await
        .context("the debug ring records browser events")?;

    browser
        .fill("[data-testid='roll-expression']", "not-dice")
        .await?;
    browser
        .click("[data-testid='roll-button']")
        .await
        .context("an invalid roll is rejected")?;
    browser
        .wait_attr(
            "[data-testid='debug-entry'][data-level='error']",
            "data-level",
            "error",
        )
        .await
        .context("the debug ring records errors")?;
    Ok(())
}

async fn hp_temp_and_rest(server: &TestServer, browser: &Browser) -> anyhow::Result<()> {
    browser.enter_campaign(server.token()).await?;
    flows::create_character(
        browser,
        &Character::new("Aria HP")
            .class("Wizard")
            .level(3)
            .max_hp(21)
            .abilities("8,14,14,18,12,10"),
    )
    .await?;

    browser.fill("[data-testid='hp-amount']", "8").await?;
    browser.click("[data-testid='damage-button']").await?;
    browser
        .wait_text("[data-testid='hp-value']", "13/21")
        .await
        .context("damage is applied to current hit points")?;

    browser.fill("[data-testid='hp-amount']", "5").await?;
    browser.click("[data-testid='temp-hp-button']").await?;
    browser
        .wait_text("[data-testid='temp-hp-value']", "5")
        .await
        .context("temporary hit points are set")?;
    browser
        .wait_text("[data-testid='hp-value']", "13/21")
        .await
        .context("temporary hit points do not change current hit points")?;

    browser.fill("[data-testid='hp-amount']", "8").await?;
    browser.click("[data-testid='damage-button']").await?;
    browser
        .wait_text("[data-testid='temp-hp-value']", "0")
        .await
        .context("damage is absorbed by temporary hit points first")?;
    browser
        .wait_text("[data-testid='hp-value']", "10/21")
        .await
        .context("only the damage above the temporary pool reaches hit points")?;

    // The amount field is part of the sheet, which remounts after every update.
    browser.fill("[data-testid='hp-amount']", "8").await?;
    browser.click("[data-testid='heal-button']").await?;
    browser
        .wait_text("[data-testid='hp-value']", "18/21")
        .await
        .context("healing raises current hit points")?;

    browser
        .click("[data-testid='long-rest-button']")
        .await
        .context("the long rest button is available")?;
    browser
        .wait_text("[data-testid='hp-value']", "21/21")
        .await
        .context("a long rest restores hit points to the maximum")?;

    browser
        .wait_attr("[data-testid='shell']", "data-error", "false")
        .await
        .context("the hit-point journey reports no errors")?;
    Ok(())
}

async fn handout_give(server: &TestServer, browser: &Browser) -> anyhow::Result<()> {
    browser.enter_campaign(server.token()).await?;
    flows::create_character(browser, &Character::new("Borin").max_hp(24)).await?;

    browser
        .click("[data-testid='handout-create'] summary")
        .await
        .context("the handout form opens")?;
    browser
        .select_value("[data-testid='handout-kind']", "item")
        .await?;
    browser
        .fill("[data-testid='handout-name']", "Healing Draught")
        .await?;
    browser
        .fill("[data-testid='handout-content']", "Potion of Healing")
        .await?;
    browser
        .click("[data-testid='create-handout-button']")
        .await
        .context("the handout is submitted")?;
    browser.wait_idle().await?;
    browser
        .wait_text("[data-testid='entity-name']", "Healing Draught")
        .await
        .context("the created handout is selected")?;

    browser
        .click("[data-testid='give-button']")
        .await
        .context("the handout is given to the only statblock")?;
    browser.wait_idle().await?;

    browser
        .click_text("[data-testid='entity-row']", "Borin")
        .await
        .context("the recipient can be selected again")?;
    browser
        .wait_text("[data-testid='entity-name']", "Borin")
        .await
        .context("the recipient sheet is selected")?;
    browser
        .wait_text("[data-testid='inventory-section']", "Potion of Healing")
        .await
        .context("the given item reaches the recipient inventory")?;

    browser
        .wait_attr("[data-testid='shell']", "data-error", "false")
        .await
        .context("the handout journey reports no errors")?;
    Ok(())
}
