//! Authorization, entity views and feed visibility (RFC `authz.md`).
//!
//! The module is pure over core documents: campaign and entity are arguments;
//! no ports, no working set, no I/O (section 6).

mod access;
mod view;

use localtt_core::types::{AnyEntity, Campaign, FeedEntry, PlayerId};

pub use self::view::{
    EntityView,
    EntityViewBody,
    FeedEntryView,
    HandoutView,
    NoteView,
    ProficienciesView,
    StatblockView,
};

/// Crate-internal: true when `viewer` is on the roster, any role (5.1). The
/// executor pre-check uses it for campaign-level commands and free rolls.
#[must_use]
#[inline]
pub(crate) fn member(campaign: &Campaign, viewer: PlayerId) -> bool {
    access::member(campaign, viewer)
}

/// Crate-internal: true when `viewer` is the campaign's DM (5.1).
#[must_use]
#[inline]
pub(crate) fn dm(campaign: &Campaign, viewer: PlayerId) -> bool {
    access::dm(campaign, viewer)
}

/// True when `viewer` may read at least one section of `entity`: roster
/// membership is required first, then DM, owner, any `Write` grant, or at
/// least one visible field from the `Read` grants (section 5.2).
#[must_use]
#[inline]
pub fn can_read(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> bool {
    access::resolve(campaign, entity, viewer)
        .is_some_and(|access| access.write || !access.visible.is_empty())
}

/// True when `viewer` may write `entity`: roster membership, then DM, owner
/// or any `Write` grant; masks are ignored (section 5.2).
#[must_use]
#[inline]
pub fn can_write(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> bool {
    access::resolve(campaign, entity, viewer).is_some_and(|access| access.write)
}

/// The viewer's mirror of one entity: mirrored headers, the visible sections,
/// the hidden set and the write flag. Never a redacted clone (ENG-8).
#[must_use]
#[inline]
pub fn view_of(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> EntityView {
    view::view_of(campaign, entity, viewer)
}

/// True when `viewer` may see the feed entry (section 5.5).
#[must_use]
#[inline]
pub fn entry_visible(campaign: &Campaign, entry: &FeedEntry, viewer: PlayerId) -> bool {
    access::entry_visible(campaign, entry, viewer)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{
            Abilities,
            AclEntry,
            AclLevel,
            Audience,
            CampaignTime,
            EntityId,
            EntityKind,
            FeedEntry,
            FeedEntryId,
            FeedPayload,
            FieldMask,
            UnixTime,
            ViewField,
        },
    };
    use proptest::prelude::*;

    use super::*;

    const HOMESTEAD: &str = "Curse of the Vault";

    /// A campaign with a DM, an owner and one unrelated member.
    fn campaign() -> (Campaign, PlayerId, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new(HOMESTEAD);
        let dm = builder.dm("Dungeon Master");
        let owner = builder.player("Owner");
        let other = builder.player("Other");
        (builder.build(), dm, owner, other)
    }

    /// A pc owned by `owner` and carrying `acl`.
    fn wizard(id: EntityId, owner: PlayerId, acl: Vec<AclEntry>) -> AnyEntity {
        let mut builder = StatblockBuilder::character();
        builder.abilities(Abilities::all(10)).hp(32, 32);
        let mut entity = builder
            .build_entity(EntityKind::Pc, id, "Aria", owner)
            .expect("abilities and hp are set");
        entity.acl = acl;
        AnyEntity::from(entity)
    }

    /// A read grant over exactly `fields`; an empty slice grants nothing.
    fn read(player: PlayerId, fields: &[ViewField]) -> AclEntry {
        let mut mask = FieldMask::default();
        for &field in fields {
            mask.insert(field);
        }
        AclEntry {
            player,
            level: AclLevel::Read,
            field_mask: Some(mask),
        }
    }

    fn all_fields() -> BTreeSet<ViewField> {
        access::ALL.into_iter().collect()
    }

    /// One feed entry with a fixed stamp and no links.
    fn entry(id: u64, audience: Audience, actor: Option<PlayerId>) -> FeedEntry {
        FeedEntry {
            id: FeedEntryId::new(id),
            ts: UnixTime::from_seconds(0),
            clock_s: CampaignTime::from_seconds(0),
            actor,
            entity: None,
            audience,
            payload: FeedPayload::Hint {
                text: format!("entry {id}"),
            },
        }
    }

    /// A field mask over exactly `fields`.
    fn field_mask(fields: &BTreeSet<ViewField>) -> FieldMask {
        let mut mask = FieldMask::default();
        for &field in fields {
            mask.insert(field);
        }
        mask
    }

    /// A read grant over exactly `mask`.
    fn read_mask(player: PlayerId, mask: &BTreeSet<ViewField>) -> AclEntry {
        AclEntry {
            player,
            level: AclLevel::Read,
            field_mask: Some(field_mask(mask)),
        }
    }

    /// One read mask of up to five sections.
    fn mask_strategy() -> impl Strategy<Value = BTreeSet<ViewField>> {
        proptest::collection::btree_set(proptest::sample::select(&access::ALL[..]), 0..5)
    }

    /// Up to four read masks, for the algebra properties.
    fn masks_strategy() -> impl Strategy<Value = Vec<BTreeSet<ViewField>>> {
        proptest::collection::vec(mask_strategy(), 0..4)
    }

    #[test]
    fn a_non_member_is_nobody_even_as_owner() {
        let (campaign, _dm, _owner, _other) = campaign();
        let outsider = PlayerId::new();
        let entity = wizard(EntityId::new(), outsider, Vec::new());

        assert!(!can_read(&campaign, &entity, outsider));
        assert!(!can_write(&campaign, &entity, outsider));
        let view = view_of(&campaign, &entity, outsider);
        assert_eq!(view.hidden, all_fields());
        assert!(view.name.is_none());
        assert!(view.ext.is_none());
        assert!(view.body.is_none());
        assert!(!view.writable);
    }

    #[test]
    fn read_masks_union_and_an_empty_mask_denies() {
        let (campaign, _dm, owner, other) = campaign();
        let id = EntityId::new();
        let entity = wizard(
            id,
            owner,
            vec![
                read(other, &[ViewField::Name]),
                read(other, &[ViewField::Hp]),
            ],
        );

        assert!(can_read(&campaign, &entity, other));
        assert!(!can_write(&campaign, &entity, other));
        let view = view_of(&campaign, &entity, other);
        let mut hidden = all_fields();
        hidden.remove(&ViewField::Name);
        hidden.remove(&ViewField::Hp);
        assert_eq!(view.hidden, hidden);
        assert_eq!(view.name.as_deref(), Some("Aria"));

        let emptied = wizard(id, owner, vec![read(other, &[])]);
        assert!(!can_read(&campaign, &emptied, other));
        let view = view_of(&campaign, &emptied, other);
        assert_eq!(view.hidden, all_fields());
        assert!(view.name.is_none());
        assert!(view.ext.is_none());
        assert!(view.body.is_none());
    }

    #[test]
    fn write_grants_ignore_their_masks() {
        let (campaign, _dm, owner, other) = campaign();
        let entity = wizard(
            EntityId::new(),
            owner,
            vec![AclEntry {
                player: other,
                level: AclLevel::Write,
                field_mask: Some(FieldMask::default()),
            }],
        );

        assert!(can_write(&campaign, &entity, other));
        let view = view_of(&campaign, &entity, other);
        assert!(view.writable);
        assert!(view.hidden.is_empty());
        assert!(view.body.is_some());
    }

    #[test]
    fn the_dm_reads_and_writes_anything() {
        let (campaign, dm, _owner, _other) = campaign();
        let entity = wizard(EntityId::new(), PlayerId::new(), Vec::new());

        assert!(can_read(&campaign, &entity, dm));
        assert!(can_write(&campaign, &entity, dm));
        let view = view_of(&campaign, &entity, dm);
        assert!(view.hidden.is_empty());
        assert!(view.writable);
    }

    #[test]
    fn feed_visibility_follows_membership_first() {
        let (campaign, dm, owner, other) = campaign();
        let outsider = PlayerId::new();
        let public = entry(1, Audience::Public, Some(other));
        let roller_and_dm = entry(2, Audience::RollerAndDm, Some(owner));
        let unowned_roller_and_dm = entry(3, Audience::RollerAndDm, None);
        let roller_only = entry(4, Audience::RollerOnly, Some(other));

        assert!(entry_visible(&campaign, &public, dm));
        assert!(entry_visible(&campaign, &public, other));
        assert!(!entry_visible(&campaign, &public, outsider));

        assert!(entry_visible(&campaign, &roller_and_dm, dm));
        assert!(entry_visible(&campaign, &roller_and_dm, owner));
        assert!(!entry_visible(&campaign, &roller_and_dm, other));
        assert!(entry_visible(&campaign, &unowned_roller_and_dm, dm));
        assert!(!entry_visible(&campaign, &unowned_roller_and_dm, other));

        assert!(entry_visible(&campaign, &roller_only, other));
        assert!(!entry_visible(&campaign, &roller_only, dm));
        assert!(!entry_visible(&campaign, &roller_only, outsider));
    }

    /// The proptest configuration.
    ///
    /// Under Miri the file-backed failure persistence cannot run (`getcwd` is
    /// not available with isolation) and the case count is cut to a handful:
    /// Miri interprets every instruction, so the full count takes minutes and
    /// adds no UB signal the handful does not. Everywhere else failing seeds
    /// persist under `proptest-regressions/`.
    fn property_config() -> ProptestConfig {
        let mut config = ProptestConfig::with_cases(if cfg!(miri) { 8 } else { 48 });
        if cfg!(miri) {
            config.failure_persistence = None;
        }
        config
    }

    proptest! {
        #![proptest_config(property_config())]

        #[test]
        fn read_grants_union_monotonically(masks in masks_strategy()) {
            let (campaign, _dm, owner, other) = campaign();
            let id = EntityId::new();
            let mut hidden = all_fields();
            let mut grants = Vec::new();
            for mask in &masks {
                grants.push(read_mask(other, mask));
                let entity = wizard(id, owner, grants.clone());
                let next = view_of(&campaign, &entity, other).hidden;
                prop_assert!(next.is_subset(&hidden));
                hidden = next;
            }
            let entity = wizard(id, owner, Vec::new());
            prop_assert_eq!(view_of(&campaign, &entity, other).hidden, all_fields());
        }

        #[test]
        fn an_empty_read_grant_is_neutral(mask in mask_strategy()) {
            let (campaign, _dm, owner, other) = campaign();
            let id = EntityId::new();
            let with = vec![read_mask(other, &mask), read_mask(other, &BTreeSet::new())];
            let without = vec![read_mask(other, &mask)];
            prop_assert_eq!(
                view_of(&campaign, &wizard(id, owner, with), other),
                view_of(&campaign, &wizard(id, owner, without), other)
            );
        }

        #[test]
        fn a_write_grant_dominates(reads in masks_strategy(), write_mask in mask_strategy()) {
            let (campaign, _dm, owner, other) = campaign();
            let mut acl: Vec<AclEntry> = reads.iter().map(|mask| read_mask(other, mask)).collect();
            acl.push(AclEntry {
                player: other,
                level: AclLevel::Write,
                field_mask: Some(field_mask(&write_mask)),
            });
            let entity = wizard(EntityId::new(), owner, acl);
            let view = view_of(&campaign, &entity, other);
            prop_assert!(view.writable);
            prop_assert!(view.hidden.is_empty());
            prop_assert!(can_write(&campaign, &entity, other));
        }
    }

    #[test]
    fn duplicate_grants_union_without_effect() {
        let (campaign, _dm, owner, other) = campaign();
        let id = EntityId::new();
        let single = wizard(
            id,
            owner,
            vec![
                read(other, &[ViewField::Name]),
                read(other, &[ViewField::Hp]),
            ],
        );
        let doubled = wizard(
            id,
            owner,
            vec![
                read(other, &[ViewField::Name]),
                read(other, &[ViewField::Hp]),
                read(other, &[ViewField::Name]),
                read(other, &[ViewField::Hp]),
            ],
        );

        assert_eq!(
            view_of(&campaign, &doubled, other),
            view_of(&campaign, &single, other)
        );
        assert!(can_read(&campaign, &doubled, other));
    }
}
