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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_rejects_a_wrong_token() {
    let mut journey = Journey::start("browser-wrong-token", "Browser Token")
        .await
        .expect("the browser-test server and browser start");
    let outcome = wrong_token(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_expires_the_session() {
    let mut journey = Journey::start("browser-session-expired", "Browser Expired")
        .await
        .expect("the browser-test server and browser start");
    let outcome = session_expiry(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_keeps_the_sheet_after_a_rejected_form() {
    let mut journey = Journey::start("browser-rejected-form", "Browser Rejected")
        .await
        .expect("the browser-test server and browser start");
    let outcome = rejected_form(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_recovers_from_an_outage() {
    let mut journey = Journey::start("browser-outage", "Browser Outage")
        .await
        .expect("the browser-test server and browser start");
    let outcome = outage_recovery(&mut journey).await;
    journey.finish(outcome).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_roster_invite_and_revoke() {
    let mut journey = Journey::start("browser-roster", "Browser Roster")
        .await
        .expect("the browser-test server and browser start");
    let outcome = roster_invite_and_revoke(&mut journey).await;
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

async fn wrong_token(journey: &mut Journey) -> anyhow::Result<()> {
    let browser = journey.browser();
    browser.wait_for("[data-testid='login']").await?;

    browser
        .click("[data-testid='connect-button']")
        .await
        .context("the empty form is submitted")?;
    browser
        .wait_text("[data-testid='login-error']", "Enter the bearer token")
        .await
        .context("an empty token is reported on the form")?;

    browser
        .fill("[data-testid='token-input']", "not-the-browser-token")
        .await?;
    browser
        .click("[data-testid='connect-button']")
        .await
        .context("the wrong token is submitted")?;
    browser
        .wait_text("[data-testid='login-error']", "rejected the token")
        .await
        .context("the rejected token is reported on the form")?;
    ensure!(
        browser
            .optional_attr("[data-testid='shell']", "data-state")
            .await?
            .is_none(),
        "a rejected token must not mount the shell"
    );
    ensure!(
        browser.local_storage("localtt.token").await?.is_none(),
        "a rejected token must not be persisted"
    );

    browser
        .fill("[data-testid='token-input']", journey.server().token())
        .await?;
    browser
        .click("[data-testid='connect-button']")
        .await
        .context("the correct token is submitted")?;
    browser
        .wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the correct token opens the campaign")?;
    browser
        .wait_text("[data-testid='campaign-name']", "Browser Token")
        .await
        .context("the login recovery reaches the right campaign")?;
    ensure!(
        browser.local_storage("localtt.token").await?.as_deref() == Some(journey.server().token()),
        "the accepted token is persisted for the next reload"
    );
    Ok(())
}

async fn session_expiry(journey: &mut Journey) -> anyhow::Result<()> {
    {
        let browser = journey.browser();
        browser.enter_campaign(journey.server().token()).await?;
        flows::create_character(browser, &Character::new("Aria Expired").max_hp(20)).await?;
        browser.fill("[data-testid='hp-amount']", "4").await?;
        browser.click("[data-testid='damage-button']").await?;
        browser
            .wait_text("[data-testid='hp-value']", "16/20")
            .await
            .context("the damage is visible before the token rotates")?;
    }

    journey
        .restart_with_token(&[], "rotated-browser-token")
        .await?;

    let browser = journey.browser();
    browser
        .wait_attr("[data-testid='shell']", "data-ws-state", "reconnecting")
        .await
        .context("the stream notices the restarted server")?;

    // The next authenticated call carries the old credential and is rejected.
    browser.fill("[data-testid='hp-amount']", "1").await?;
    browser
        .click("[data-testid='damage-button']")
        .await
        .context("a command is sent with the expired token")?;
    browser
        .wait_text("[data-testid='login-error']", "rejected the token")
        .await
        .context("a rejected call returns to the login form")?;
    ensure!(
        browser
            .optional_attr("[data-testid='shell']", "data-state")
            .await?
            .is_none(),
        "an expired session must not leave the shell mounted"
    );
    ensure!(
        browser.local_storage("localtt.token").await?.is_none(),
        "an expired token must be cleared"
    );

    browser
        .fill("[data-testid='token-input']", "rotated-browser-token")
        .await?;
    browser
        .click("[data-testid='connect-button']")
        .await
        .context("the rotated token is submitted")?;
    browser
        .wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the rotated token opens the campaign")?;
    browser
        .wait_text("[data-testid='hp-value']", "16/20")
        .await
        .context("the campaign survived the token rotation")?;
    Ok(())
}

async fn rejected_form(journey: &mut Journey) -> anyhow::Result<()> {
    let browser = journey.browser();
    browser.enter_campaign(journey.server().token()).await?;
    flows::create_character(browser, &Character::new("Aria Kept").max_hp(16)).await?;

    browser.fill("[data-testid='character-name']", "").await?;
    browser
        .click("[data-testid='create-character-button']")
        .await
        .context("an empty name is submitted")?;
    browser
        .wait_text("[data-testid='error']", "name is required")
        .await
        .context("the form explains what is missing")?;
    browser
        .wait_text("[data-testid='entity-name']", "Aria Kept")
        .await
        .context("the rejected form keeps the sheet on screen")?;
    browser
        .wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the rejected form keeps the campaign loaded")?;

    browser
        .click("[data-testid='error-dismiss']")
        .await
        .context("the error banner can be dismissed")?;
    ensure!(
        browser
            .optional_text("[data-testid='error']")
            .await?
            .is_none(),
        "dismissing the banner clears it"
    );

    browser
        .click("[data-testid='create-character-button']")
        .await
        .context("the rejected form is submitted again")?;
    browser
        .wait_text("[data-testid='error']", "name is required")
        .await
        .context("the retry reports the same problem")?;

    browser
        .fill("[data-testid='character-name']", "Borin Kept")
        .await?;
    browser
        .click("[data-testid='create-character-button']")
        .await
        .context("the corrected form is submitted")?;
    browser
        .wait_text("[data-testid='entity-name']", "Borin Kept")
        .await
        .context("the corrected form creates the character")?;
    browser
        .wait_attr("[data-testid='shell']", "data-error", "false")
        .await
        .context("a successful retry clears the error")?;
    Ok(())
}

async fn outage_recovery(journey: &mut Journey) -> anyhow::Result<()> {
    {
        let browser = journey.browser();
        browser.enter_campaign(journey.server().token()).await?;
        flows::create_character(browser, &Character::new("Aria Outage").max_hp(24)).await?;
        browser.fill("[data-testid='hp-amount']", "6").await?;
        browser.click("[data-testid='damage-button']").await?;
        browser
            .wait_text("[data-testid='hp-value']", "18/24")
            .await
            .context("the damage is visible before the outage")?;
    }

    journey.stop().await?;

    let browser = journey.browser();
    browser
        .wait_attr("[data-testid='shell']", "data-ws-state", "reconnecting")
        .await
        .context("the client notices the stream is down")?;
    browser.fill("[data-testid='hp-amount']", "1").await?;
    browser
        .click("[data-testid='damage-button']")
        .await
        .context("a command is sent while the server is down")?;
    browser
        .wait_attr("[data-testid='shell']", "data-error", "true")
        .await
        .context("the failed command reports an error")?;
    browser
        .wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the outage keeps the sheet on screen")?;

    journey.resume(&[]).await?;

    let browser = journey.browser();
    browser
        .wait_attr_contains("[data-testid='shell']", "data-ws-state", "live")
        .await
        .context("the stream reconnects after the outage")?;
    browser
        .wait_attr("[data-testid='shell']", "data-error", "false")
        .await
        .context("the reconnect resync clears the outage error")?;
    browser
        .wait_text("[data-testid='hp-value']", "18/24")
        .await
        .context("the sheet still shows the pre-outage hit points")?;
    Ok(())
}

async fn roster_invite_and_revoke(journey: &mut Journey) -> anyhow::Result<()> {
    let dm = journey.browser();
    dm.enter_campaign(journey.server().token()).await?;
    dm.wait_for("[data-testid='roster-button']")
        .await
        .context("the DM sees the roster control")?;
    dm.wait_for("[data-testid='save-button']")
        .await
        .context("the DM sees the save control")?;
    dm.wait_for("[data-testid='debug-button']")
        .await
        .context("the DM sees the diagnostics control")?;
    dm.click("[data-testid='roster-button']")
        .await
        .context("the roster panel opens")?;
    dm.wait_for("[data-testid='roster-panel']").await?;

    dm.fill("[data-testid='roster-name']", "Bea Player").await?;
    dm.click("[data-testid='roster-create']")
        .await
        .context("the new seat is created")?;
    dm.wait_text(
        "[data-testid='roster-row'][data-role='player'] [data-testid='invite-state']",
        "invite active",
    )
    .await
    .context("the new seat has an invite")?;
    let invite_url = dm.input_value("[data-testid='invite-url']").await?;
    ensure!(
        invite_url.contains("#token="),
        "the invite URL carries the token fragment: {invite_url:?}"
    );

    // Opening the link while a session is stored must ask before replacing it.
    dm.open(&invite_url).await?;
    dm.wait_for("[data-testid='join-prompt']")
        .await
        .context("an existing session prompts before the link is used")?;
    ensure!(
        !dm.current_url().await?.contains("#token="),
        "the fragment is stripped from the address bar"
    );
    dm.click("[data-testid='join-dismiss']")
        .await
        .context("the invite prompt is dismissed")?;
    dm.wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the existing session keeps running")?;

    if dm
        .optional_text("[data-testid='roster-panel']")
        .await?
        .is_none()
    {
        dm.click("[data-testid='roster-button']")
            .await
            .context("the roster panel reopens")?;
    }
    dm.wait_for("[data-testid='roster-panel']").await?;

    let mut player = Browser::launch(journey.server().base_url()).await?;
    let result = join_then_revoke(dm, &player, &invite_url).await;
    let shutdown = player.shutdown().await;
    result?;
    shutdown?;

    // Removal needs two clicks; the first only asks for confirmation.
    let row = "[data-testid='roster-row'][data-role='player']";
    dm.click(&format!("{row} [data-testid='remove-seat']"))
        .await
        .context("the first remove click asks for confirmation")?;
    dm.wait_text(
        &format!("{row} [data-testid='remove-seat']"),
        "Confirm remove",
    )
    .await?;
    dm.click(&format!("{row} [data-testid='cancel-remove']"))
        .await
        .context("the removal can be cancelled")?;
    dm.wait_text(&format!("{row} [data-testid='remove-seat']"), "Remove")
        .await?;
    dm.click(&format!("{row} [data-testid='remove-seat']"))
        .await?;
    dm.click(&format!("{row} [data-testid='remove-seat']"))
        .await
        .context("the confirmed removal runs")?;
    dm.wait_count(row, 0)
        .await
        .context("the removed seat leaves the roster")?;
    Ok(())
}

async fn join_then_revoke(dm: &Browser, player: &Browser, invite_url: &str) -> anyhow::Result<()> {
    player.open(invite_url).await?;
    player
        .wait_attr("[data-testid='shell']", "data-state", "ready")
        .await
        .context("the invite link opens the campaign")?;
    player
        .wait_text("[data-testid='campaign-name']", "Browser Roster")
        .await?;
    ensure!(
        !player.current_url().await?.contains("#token="),
        "the invite fragment is stripped after it is consumed"
    );
    ensure!(
        player
            .optional_text("[data-testid='roster-button']")
            .await?
            .is_none(),
        "a player must not see the roster control"
    );
    ensure!(
        player
            .optional_text("[data-testid='save-button']")
            .await?
            .is_none(),
        "a player must not see the save control"
    );
    ensure!(
        player
            .optional_text("[data-testid='debug-button']")
            .await?
            .is_none(),
        "a player must not see the diagnostics control"
    );

    dm.click("[data-testid='roster-row'][data-role='player'] [data-testid='revoke-invite']")
        .await
        .context("the DM revokes the player invite")?;
    player
        .wait_text("[data-testid='login-error']", "revoked this session")
        .await
        .context("the revoked stream returns the player to the login form")?;
    ensure!(
        player
            .optional_attr("[data-testid='shell']", "data-state")
            .await?
            .is_none(),
        "a revoked player must not keep the shell mounted"
    );
    ensure!(
        player.local_storage("localtt.token").await?.is_none(),
        "a revoked token is cleared from storage"
    );
    dm.wait_text(
        "[data-testid='roster-row'][data-role='player'] [data-testid='invite-state']",
        "no invite",
    )
    .await
    .context("the roster reports the revoked invite")?;
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
