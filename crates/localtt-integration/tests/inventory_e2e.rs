//! End-to-end inventory test: real SQLite, executor, item commands and a
//! handout copy.

use localtt_adapter::store::sqlite::SqliteStore;
use localtt_core::{
    builders::{CampaignBuilder, StatblockBuilder},
    types::{
        Abilities,
        AnyEntity,
        Campaign,
        DiceResult,
        Entity,
        EntityId,
        EntityKind,
        Ext,
        HandoutBody,
        HandoutContent,
        InlineItem,
        ItemKind,
        ItemState,
        UnixTime,
    },
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
            reason: "the inventory test rolls nothing".to_owned(),
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

fn inline(name: &str) -> InlineItem {
    InlineItem {
        name: name.to_owned(),
        description: String::new(),
        kind: ItemKind::Gear,
        requires_attunement: false,
        weight_oz: None,
        value: None,
        properties: None,
        modifiers: Vec::new(),
        resource: None,
        ext: Ext::new(),
    }
}

#[test]
fn items_edits_and_a_given_handout_survive_a_store_reopen() {
    let dir = TempDir::new().expect("a temporary directory");
    let path = dir.path().join("campaign.sqlite");
    let mut store = SqliteStore::open(&path).expect("the store opens");
    let mut builder = CampaignBuilder::new("Inventory");
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

    let mut character = StatblockBuilder::character();
    character.abilities(Abilities::all(12)).hp(12, 12);
    let character = character
        .build_entity(EntityKind::Pc, EntityId::new(), "Aria", dm)
        .expect("abilities and hp are set");
    let character_id = character.id;
    executor
        .execute_as(
            dm,
            &Command::CreateEntity {
                entity: Box::new(AnyEntity::from(character)),
            },
        )
        .expect("the DM creates a character");

    executor
        .execute_as(
            dm,
            &Command::AddInventoryItem {
                entity: character_id,
                item: Box::new(localtt_core::types::InventoryItem::Inline(inline("Rope"))),
                qty: 2,
                state: ItemState::Equipped,
                attuned: false,
                notes: "50 feet".to_owned(),
                expected_rev: Some(1),
            },
        )
        .expect("the DM adds an item");

    let handout = Entity::builder(
        EntityKind::Handout,
        EntityId::new(),
        "Loot",
        dm,
        HandoutBody {
            content: HandoutContent::Item {
                item: inline("Potion of Healing"),
            },
        },
    )
    .build()
    .expect("a handout kind pairs with a handout body");
    let handout_id = handout.id;
    executor
        .execute_as(
            dm,
            &Command::CreateEntity {
                entity: Box::new(AnyEntity::from(handout)),
            },
        )
        .expect("the DM creates a handout");
    executor
        .execute_as(
            dm,
            &Command::GiveHandout {
                handout: handout_id,
                recipient: character_id,
                expected_rev: Some(2),
            },
        )
        .expect("the DM gives the item");
    drop(executor);
    drop(store);

    let reopened = SqliteStore::open(&path).expect("the store reopens");
    let loaded = reopened.load().expect("the campaign reloads");
    let character = loaded
        .entities
        .iter()
        .find(|entity| entity.id() == character_id)
        .expect("the character is stored");
    let AnyEntity::Statblock(character) = character else {
        panic!("the character is a statblock");
    };
    assert_eq!(character.rev, 3);
    assert_eq!(character.body.inventory.len(), 2);
    assert_eq!(
        character.body.inventory[0].item,
        localtt_core::types::InventoryItem::Inline(inline("Rope"))
    );
    assert_eq!(character.body.inventory[0].qty, 2);
    assert_eq!(character.body.inventory[0].state, ItemState::Equipped);
    assert_eq!(character.body.inventory[0].notes, "50 feet");
    assert_eq!(
        character.body.inventory[1].item,
        localtt_core::types::InventoryItem::Inline(inline("Potion of Healing"))
    );
    let handout = loaded
        .entities
        .iter()
        .find(|entity| entity.id() == handout_id)
        .expect("the handout is stored");
    assert_eq!(handout.rev(), 1);
}
