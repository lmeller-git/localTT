//! Browser journeys through the real HTTP/WS server and Trunk assets.
//!
//! These tests need Firefox and geckodriver, so they are ignored by the normal
//! `just test` gate. Run them with `just e2e`.

mod support;

use anyhow::{Context, ensure};
use localtt_core::types::EntityId;
use localtt_engine::commands::Command;
use support::{
    browser::Browser,
    flows::{self, Character},
    journey::Journey,
};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_login_create_and_save() {
    let mut journey = Journey::start("browser-smoke", "Browser E2E")
        .await
        .expect("the browser-test server and browser start");
    let outcome = session_smoke(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_hp_temp_and_long_rest() {
    let mut journey = Journey::start("browser-hp-rest", "Browser HP")
        .await
        .expect("the browser-test server and browser start");
    let outcome = hp_temp_and_rest(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_handout_give() {
    let mut journey = Journey::start("browser-handout", "Browser Handout")
        .await
        .expect("the browser-test server and browser start");
    let outcome = handout_give(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_reconnects_and_resyncs() {
    let mut journey = Journey::start("browser-resync", "Browser Resync")
        .await
        .expect("the browser-test server and browser start");
    let outcome = reconnect_resync(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_restart_keeps_the_campaign() {
    let mut journey = Journey::start("browser-restart", "Browser Restart")
        .await
        .expect("the browser-test server and browser start");
    let outcome = restart_persistence(&mut journey).await;
    journey.finish(outcome).await;
}

async fn session_smoke(journey: &mut Journey) -> anyhow::Result<()> {
    let browser = journey.browser();
    browser.enter_campaign(journey.server().token()).await?;

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

async fn hp_temp_and_rest(journey: &mut Journey) -> anyhow::Result<()> {
    let browser = journey.browser();
    browser.enter_campaign(journey.server().token()).await?;
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

async fn handout_give(journey: &mut Journey) -> anyhow::Result<()> {
    let browser = journey.browser();
    browser.enter_campaign(journey.server().token()).await?;
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

async fn reconnect_resync(journey: &mut Journey) -> anyhow::Result<()> {
    let entity = {
        let browser = journey.browser();
        browser.enter_campaign(journey.server().token()).await?;
        flows::create_character(browser, &Character::new("Aria Resync").max_hp(20)).await?;
        browser.fill("[data-testid='hp-amount']", "5").await?;
        browser.click("[data-testid='damage-button']").await?;
        browser
            .wait_text("[data-testid='hp-value']", "15/20")
            .await
            .context("the live stream applies the first damage")?;
        entity_id(browser).await?
    };

    // The damage lands while no client is subscribed, so only a resync can
    // show it after the socket comes back.
    journey
        .restart(&[Command::ApplyDamage {
            entity,
            amount: 5,
            expected_rev: None,
        }])
        .await?;

    let browser = journey.browser();
    browser
        .wait_attr("[data-testid='shell']", "data-ws-state", "live")
        .await
        .context("the browser reconnects to the restarted server")?;
    browser
        .wait_text("[data-testid='hp-value']", "10/20")
        .await
        .context("the reconnect resyncs the sheet over REST")?;
    browser
        .wait_attr("[data-testid='shell']", "data-error", "false")
        .await
        .context("the resync reports no errors")?;
    Ok(())
}

async fn restart_persistence(journey: &mut Journey) -> anyhow::Result<()> {
    {
        let browser = journey.browser();
        browser.enter_campaign(journey.server().token()).await?;
        flows::create_character(browser, &Character::new("Aria Persist").max_hp(24)).await?;
        browser.fill("[data-testid='hp-amount']", "6").await?;
        browser.click("[data-testid='damage-button']").await?;
        browser
            .wait_text("[data-testid='hp-value']", "18/24")
            .await
            .context("the damage is visible before the restart")?;
        browser
            .click("[data-testid='save-button']")
            .await
            .context("the save button is available")?;
        browser
            .wait_text("[data-testid='status']", "saved")
            .await
            .context("the save completes")?;
    }

    journey.restart(&[]).await?;

    let browser = journey.browser();
    let base_url = journey.server().base_url().to_owned();
    browser
        .open(&base_url)
        .await
        .context("the browser reloads against the restarted server")?;
    browser
        .wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the stored token reconnects without a second login")?;
    browser
        .wait_text("[data-testid='campaign-name']", "Browser Restart")
        .await
        .context("the campaign survives the restart")?;
    browser
        .wait_text("[data-testid='entity-name']", "Aria Persist")
        .await
        .context("the character survives the restart")?;
    browser
        .wait_text("[data-testid='hp-value']", "18/24")
        .await
        .context("the hit points survive the restart")?;
    Ok(())
}

async fn entity_id(browser: &Browser) -> anyhow::Result<EntityId> {
    let raw = browser
        .optional_attr("[data-testid='entity-detail']", "data-entity-id")
        .await?
        .context("the selected entity exposes its id")?;
    let uuid = Uuid::parse_str(&raw).with_context(|| format!("parsing entity id {raw:?}"))?;
    Ok(EntityId::from_uuid(uuid))
}
