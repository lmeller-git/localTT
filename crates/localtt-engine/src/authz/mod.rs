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
    todo!()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use localtt_core::{
        builders::{CampaignBuilder, StatblockBuilder},
        types::{Abilities, AclEntry, AclLevel, EntityId, EntityKind, FieldMask, ViewField},
    };

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
}
