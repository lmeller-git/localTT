//! Browser integration smoke through the real HTTP/WS server and Trunk assets.
//!
//! These tests need Firefox and geckodriver, so they are ignored by the normal
//! `just test` gate. Run them with `just e2e`.

mod support;

use anyhow::{Context, ensure};
use support::{browser::Browser, server::TestServer};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Firefox and geckodriver; run with `just e2e`"]
async fn browser_login_create_and_save() {
    let mut server = TestServer::start("Browser E2E")
        .await
        .expect("the browser-test server starts");
    let mut browser = match Browser::launch(server.base_url()).await {
        Ok(browser) => browser,
        Err(error) => {
            let _ = server.shutdown().await;
            panic!("the browser starts: {error:#}");
        }
    };

    if let Err(error) = run_smoke(&server, &browser).await {
        browser.capture_failure("browser-smoke").await;
        let _ = browser.shutdown().await;
        let _ = server.shutdown().await;
        panic!("browser smoke failed: {error:#}");
    }

    browser
        .shutdown()
        .await
        .expect("the browser session closes");
    server.shutdown().await.expect("the test server stops");
}

async fn run_smoke(server: &TestServer, browser: &Browser) -> anyhow::Result<()> {
    browser.login(server.token()).await?;
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

    browser
        .click("[data-testid='character-create'] summary")
        .await?;
    browser
        .fill("[data-testid='character-name']", "Aria Test")
        .await?;
    browser
        .fill("[data-testid='character-class']", "Wizard")
        .await?;
    browser.fill("[data-testid='character-level']", "3").await?;
    browser
        .fill("[data-testid='character-max-hp']", "21")
        .await?;
    browser
        .fill("[data-testid='character-abilities']", "8,14,14,18,12,10")
        .await?;
    browser
        .click("[data-testid='create-character-button']")
        .await?;

    browser
        .wait_text("[data-testid='entity-name']", "Aria Test")
        .await
        .context("the created character is selected")?;
    browser
        .wait_text("[data-testid='hp-value']", "21/21")
        .await
        .context("the derived sheet shows the created character's HP")?;
    let detail = browser
        .wait_attr("[data-testid='entity-detail']", "data-writable", "true")
        .await?;
    let entity_id = detail
        .attr("data-entity-id")
        .await?
        .context("the selected entity exposes its id")?;
    ensure!(!entity_id.is_empty(), "the selected entity id is empty");

    browser
        .click("[data-testid='save-button']")
        .await
        .context("the save button is available")?;
    browser
        .wait_attr("[data-testid='shell']", "data-busy", "false")
        .await
        .context("the save finishes")?;
    browser
        .wait_text("[data-testid='status']", "saved")
        .await
        .context("the save status is visible")?;
    Ok(())
}
