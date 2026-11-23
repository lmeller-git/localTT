//! End-to-end coverage for the SQLite store behind the actor.

use std::sync::Arc;

use localtt_adapter::{reference::FixtureSource, store::SqliteStore};
use localtt_engine::{commands::Command, content::ReferenceSource, ports::store::CampaignStore};
use localtt_server::{
    app::AppHandle,
    env::ServerEnv,
    session::{CreateCampaign, start},
};
use tempfile::TempDir;

#[tokio::test]
async fn the_sqlite_store_commits_through_the_actor_and_reopens() {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("campaign.sqlite");
    let mut store = SqliteStore::open(&path).expect("the store opens");
    let create = CreateCampaign::new("Actor campaign", "DM");
    let started = start(&mut store, Some(&create), None).expect("the campaign is created");
    let content: Arc<dyn ReferenceSource> = Arc::new(FixtureSource::new());
    let app = AppHandle::start(
        store,
        started.snapshot,
        Box::new(ServerEnv::new(Arc::clone(&content))),
        content,
    )
    .expect("the actor starts");

    app.execute(started.actor, Command::AdvanceClock { by_seconds: 3_600 })
        .await
        .expect("the DM advances the clock");
    app.save().await.expect("the WAL checkpoints");
    app.shutdown().await.expect("the actor stops");

    let reopened = SqliteStore::open(&path).expect("the write reopens");
    let snapshot = CampaignStore::load(&reopened).expect("the campaign is durable");
    assert_eq!(snapshot.campaign.clock.now_s.seconds(), 3_600);
}
