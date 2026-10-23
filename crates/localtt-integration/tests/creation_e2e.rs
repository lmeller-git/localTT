//! End-to-end creation test: a real SQLite store and executor create a
//! character through the same command path the TUI and server use.

use localtt_adapter::store::sqlite::SqliteStore;
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder},
    types::{Abilities, AnyEntity, Campaign, DiceResult, EntityId, EntityKind, UnixTime},
};
use localtt_engine::{
    commands::{Command, CommandEnv},
    dice::RollPlan,
    error::EngineError,
    executor::Executor,
    ports::store::{CampaignStore, StatePatch},
    state::LoadedCampaign,
};
use tempfile::TempDir;

const FIXED_TIME: i64 = 1_750_000_000;

struct Env;

impl CommandEnv for Env {
    fn now(&self) -> UnixTime {
        UnixTime::from_seconds(FIXED_TIME)
    }

    fn roll(&mut self, _plan: &RollPlan) -> Result<DiceResult, EngineError> {
        Err(EngineError::Environment {
            reason: "the creation test rolls nothing".to_owned(),
        })
    }
}

fn campaign_only(campaign: &Campaign) -> StatePatch<'_> {
    StatePatch {
        campaign: Some(campaign),
        upserts: Vec::new(),
        removals: Vec::new(),
        feed_appends: Vec::new(),
        feed_keep_from: None,
    }
}

#[test]
fn a_created_character_survives_a_store_reopen() {
    let dir = TempDir::new().expect("a temporary directory");
    let path = dir.path().join("campaign.sqlite");
    let mut store = SqliteStore::open(&path).expect("the store opens");
    let mut builder = CampaignBuilder::new("Creation");
    let dm = builder.dm("DM");
    let campaign = builder.build();
    store
        .apply(&campaign_only(&campaign))
        .expect("the campaign seeds the store");
    let snapshot = store.load().expect("the campaign loads");
    let mut executor = Executor::new(
        Box::new(LoadedCampaign::from_snapshot(snapshot)),
        &mut store,
        Box::new(Env),
    );

    let mut builder = StatblockBuilder::character();
    builder.abilities(Abilities::all(12)).hp(12, 12);
    let entity = builder
        .build_entity(EntityKind::Pc, EntityId::new(), "Aria", dm)
        .expect("abilities and hp are set");
    let id = entity.id;

    let executed = executor
        .execute_as(
            dm,
            &Command::CreateEntity {
                entity: Box::new(AnyEntity::from(entity)),
            },
        )
        .expect("the DM creates a character");
    assert_eq!(executed.warnings, Vec::new());
    drop(executor);
    drop(store);

    let reopened = SqliteStore::open(&path).expect("the store reopens");
    let loaded = reopened.load().expect("the campaign reloads");
    let created = loaded
        .entities
        .iter()
        .find(|entity| entity.id() == id)
        .expect("the created character is stored");
    assert_eq!(created.rev(), 1);
    assert_eq!(created.owner(), dm);
    assert_eq!(created.name(), "Aria");
    assert_eq!(created.kind(), EntityKind::Pc);
    assert_eq!(created.created_at(), UnixTime::from_seconds(FIXED_TIME));
}
