//! Campaign startup: load an existing campaign or bootstrap one (F4).

use localtt_core::{
    builders::CampaignBuilder,
    types::{PlayerId, Role},
};
use localtt_engine::ports::store::{CampaignSnapshot, CampaignStore, StatePatch, StoreFault};
use thiserror::Error;

use crate::identity::{AuthIndex, AuthIndexError};

/// The campaign to create when the store is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateCampaign {
    /// Display name of the new campaign.
    pub name: String,
    /// Display name of the DM seat.
    pub dm_name: String,
}

impl CreateCampaign {
    /// Builds the create request.
    #[must_use]
    #[inline]
    pub fn new(name: impl Into<String>, dm_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            dm_name: dm_name.into(),
        }
    }
}

/// The opened campaign and the local actor's seat.
#[derive(Clone, Debug)]
pub struct StartedCampaign {
    /// The loaded campaign and its entities/feed.
    pub snapshot: CampaignSnapshot,
    /// The local DM (or the explicitly selected roster member).
    pub actor: PlayerId,
}

/// Why startup could not open a campaign.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StartError {
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreFault),
    /// The store is empty and no create request was supplied.
    #[error("the store holds no campaign; pass --create")]
    MissingCampaign,
    /// `--create` was supplied for a non-empty store.
    #[error("the store already holds a campaign; omit --create")]
    CampaignExists,
    /// No roster member has the DM role.
    #[error("the campaign has no DM seat")]
    NoDm,
    /// More than one DM exists, so the seat is ambiguous.
    #[error("the campaign has more than one DM; pass --actor")]
    MultipleDms,
    /// An explicit actor is not on the roster.
    #[error("player {0} is not on the campaign roster")]
    ActorNotOnRoster(PlayerId),
    /// An explicit actor is on the roster but is not a DM.
    #[error("player {0} is not a DM seat")]
    ActorNotDm(PlayerId),
    /// The roster cannot be indexed for authentication.
    #[error(transparent)]
    Roster(#[from] AuthIndexError),
}

/// Loads the campaign, or creates one with a DM seat when `create` is set.
///
/// On an empty store without `create`, returns [`StartError::MissingCampaign`].
/// With `create` on a non-empty store, returns [`StartError::CampaignExists`],
/// so a mistyped flag cannot silently adopt or overwrite an existing file.
///
/// # Errors
///
/// Propagates store faults and reports an empty/ambiguous campaign or an
/// explicit actor outside the roster.
#[inline]
pub fn start<S: CampaignStore>(
    store: &mut S,
    create: Option<&CreateCampaign>,
    actor: Option<PlayerId>,
) -> Result<StartedCampaign, StartError> {
    let snapshot = match store.load() {
        Ok(snapshot) => {
            if create.is_some() {
                return Err(StartError::CampaignExists);
            }
            snapshot
        }
        Err(StoreFault::NoCampaign) => {
            let Some(create) = create else {
                return Err(StartError::MissingCampaign);
            };
            let mut builder = CampaignBuilder::new(create.name.clone());
            builder.dm(create.dm_name.clone());
            let campaign = builder.build();
            let patch = StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            };
            store.apply(&patch)?;
            store.load()?
        }
        Err(fault) => return Err(fault.into()),
    };

    // Fail closed on a duplicated roster before any token can authenticate.
    AuthIndex::build(&snapshot.campaign)?;

    let actor = match actor {
        Some(actor) => {
            let seat = snapshot
                .campaign
                .players
                .iter()
                .find(|player| player.id == actor)
                .ok_or(StartError::ActorNotOnRoster(actor))?;
            if seat.role != Role::Dm {
                return Err(StartError::ActorNotDm(actor));
            }
            actor
        }
        None => {
            let mut dms = snapshot
                .campaign
                .players
                .iter()
                .filter(|player| player.role == Role::Dm)
                .map(|player| player.id);
            let Some(dm) = dms.next() else {
                return Err(StartError::NoDm);
            };
            if dms.next().is_some() {
                return Err(StartError::MultipleDms);
            }
            dm
        }
    };

    Ok(StartedCampaign { snapshot, actor })
}

#[cfg(test)]
mod tests {
    use localtt_adapter::store::MemoryStore;
    use localtt_core::{
        builders::CampaignBuilder,
        types::{PlayerId, Role, TokenHash},
    };
    use localtt_engine::ports::store::{CampaignStore, StatePatch};

    use super::{CreateCampaign, StartError, start};
    use crate::identity::AuthIndexError;

    fn create(name: &str) -> CreateCampaign {
        CreateCampaign::new(name, "Dungeon Master")
    }

    #[test]
    fn an_empty_store_needs_an_explicit_create() {
        let mut store = MemoryStore::new();
        assert!(matches!(
            start(&mut store, None, None),
            Err(StartError::MissingCampaign)
        ));
    }

    #[test]
    fn create_persists_the_campaign_and_selects_the_only_dm() {
        let mut store = MemoryStore::new();
        let started = start(&mut store, Some(&create("Curse of the Vault")), None)
            .expect("an empty store creates the campaign");

        assert_eq!(started.snapshot.campaign.name, "Curse of the Vault");
        assert_eq!(started.snapshot.campaign.players.len(), 1);
        assert_eq!(started.snapshot.campaign.players[0].role, Role::Dm);
        assert_eq!(started.snapshot.campaign.players[0].id, started.actor);

        let loaded = start(&mut store, None, None).expect("the campaign now loads");
        assert_eq!(loaded.snapshot, started.snapshot);
        assert_eq!(loaded.actor, started.actor);

        assert!(matches!(
            start(&mut store, Some(&create("Another")), None),
            Err(StartError::CampaignExists)
        ));
    }

    #[test]
    fn an_explicit_actor_must_be_on_the_roster() {
        let mut store = MemoryStore::new();
        start(&mut store, Some(&create("Curse of the Vault")), None)
            .expect("the campaign is created");

        assert!(matches!(
            start(&mut store, None, Some(PlayerId::new())),
            Err(StartError::ActorNotOnRoster(_))
        ));
    }

    #[test]
    fn an_explicit_actor_must_be_a_dm() {
        // Arrange: one DM and one player seat.
        let mut builder = CampaignBuilder::new("Mixed roster");
        builder.dm("DM");
        let player = builder.player("Alex");
        let campaign = builder.build();
        let mut store = MemoryStore::new();
        store
            .apply(&StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("the campaign is valid");

        // Act
        let result = start(&mut store, None, Some(player));

        // Assert
        assert!(matches!(
            result,
            Err(StartError::ActorNotDm(seat)) if seat == player
        ));
    }

    #[test]
    fn multiple_dms_require_an_actor() {
        let mut builder = CampaignBuilder::new("Two DMs");
        let first = builder.dm("First");
        let second = builder.dm("Second");
        let campaign = builder.build();
        let mut store = MemoryStore::new();
        store
            .apply(&StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("the campaign is valid");

        assert!(matches!(
            start(&mut store, None, None),
            Err(StartError::MultipleDms)
        ));
        let started = start(&mut store, None, Some(first)).expect("the first DM is on the roster");
        assert_eq!(started.actor, first);
        let started =
            start(&mut store, None, Some(second)).expect("the second DM is on the roster");
        assert_eq!(started.actor, second);
    }

    #[test]
    fn a_store_without_a_dm_is_refused() {
        let mut builder = CampaignBuilder::new("No DM");
        builder.player("Only a player");
        let campaign = builder.build();
        let mut store = MemoryStore::new();
        store
            .apply(&StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("the campaign is valid");

        assert!(matches!(
            start(&mut store, None, None),
            Err(StartError::NoDm)
        ));
    }

    #[test]
    fn a_duplicate_token_hash_fails_startup() {
        // Arrange: two seats claim the same full SHA-256 hash.
        let mut builder = CampaignBuilder::new("Duplicate");
        let first = builder.dm("DM");
        let second = builder.player("Alex");
        let mut campaign = builder.build();
        let hash = TokenHash::parse(&"a".repeat(64)).expect("64 hex characters are valid");
        for seat in &mut campaign.players {
            seat.token_hash = Some(hash.clone());
        }
        let mut store = MemoryStore::new();
        store
            .apply(&StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("the campaign is valid");

        // Act
        let result = start(&mut store, None, None);

        // Assert
        assert!(matches!(
            result,
            Err(StartError::Roster(AuthIndexError::DuplicateHash { first: a, second: b }))
                if (a, b) == (first, second)
        ));
    }

    #[test]
    fn a_short_legacy_hash_does_not_fail_startup() {
        // Arrange: the seat carries a hash the index cannot use.
        let mut builder = CampaignBuilder::new("Legacy");
        let dm = builder.dm("DM");
        let mut campaign = builder.build();
        campaign
            .players
            .iter_mut()
            .find(|seat| seat.id == dm)
            .expect("the DM exists")
            .token_hash = Some(TokenHash::parse("0a1b").expect("short hex parses"));
        let mut store = MemoryStore::new();
        store
            .apply(&StatePatch {
                campaign: Some(&campaign),
                upserts: Vec::new(),
                removals: Vec::new(),
                feed_appends: Vec::new(),
                feed_keep_from: None,
            })
            .expect("the campaign is valid");

        // Act
        let started = start(&mut store, None, None);

        // Assert
        assert!(started.is_ok(), "legacy hashes are skipped, not fatal");
    }
}
