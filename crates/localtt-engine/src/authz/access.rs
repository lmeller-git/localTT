//! Membership, roles, the grant/mask algebra and the feed audience predicate
//! (RFC `authz.md` 5.1, 5.2, 5.5 and section 6).

use std::collections::BTreeSet;

use localtt_core::types::{
    AclLevel,
    AnyEntity,
    Audience,
    Campaign,
    FeedEntry,
    PlayerId,
    Role,
    ViewField,
};

/// The 20-section universe, mirrored exhaustively from core [`ViewField`]; the
/// `slot` guard in the tests makes a core addition a compile error (RFC
/// `authz.md` 5.2, OQ-3).
pub(super) const ALL: [ViewField; 20] = [
    ViewField::Name,
    ViewField::Identity,
    ViewField::Abilities,
    ViewField::Saves,
    ViewField::Skills,
    ViewField::Proficiencies,
    ViewField::Hp,
    ViewField::Ac,
    ViewField::Speed,
    ViewField::DeathSaves,
    ViewField::Coins,
    ViewField::Overrides,
    ViewField::Resources,
    ViewField::Spells,
    ViewField::Inventory,
    ViewField::Attacks,
    ViewField::Features,
    ViewField::Statuses,
    ViewField::Notes,
    ViewField::Ext,
];

/// One member viewer's resolved access to one entity (RFC `authz.md` 5.2).
pub(super) struct Access {
    /// The viewer may write; a write grant implies every section.
    pub(super) write: bool,
    /// The visible sections.
    pub(super) visible: BTreeSet<ViewField>,
}

/// True when `viewer` is on the roster, any role (5.1).
pub(super) fn member(campaign: &Campaign, viewer: PlayerId) -> bool {
    campaign.players.iter().any(|player| player.id == viewer)
}

/// True when `viewer` is the campaign's DM: membership first, then
/// [`Role::Dm`] (5.1).
pub(super) fn dm(campaign: &Campaign, viewer: PlayerId) -> bool {
    member(campaign, viewer)
        && campaign
            .players
            .iter()
            .any(|player| player.id == viewer && player.role == Role::Dm)
}

/// Resolves one member viewer's access to one entity; `None` for a
/// non-member, who is nobody here even as the document's owner (5.1, 5.2).
pub(super) fn resolve(campaign: &Campaign, entity: &AnyEntity, viewer: PlayerId) -> Option<Access> {
    if !member(campaign, viewer) {
        return None;
    }
    let mut write = dm(campaign, viewer) || entity.owner() == viewer;
    let mut visible = BTreeSet::new();
    for entry in entity.acl() {
        if entry.player != viewer {
            continue;
        }
        match entry.level {
            AclLevel::Write => write = true,
            AclLevel::Read => match &entry.field_mask {
                None => visible.extend(ALL),
                Some(mask) => visible.extend(mask.iter()),
            },
        }
    }
    if write {
        visible.extend(ALL);
    }
    Some(Access { write, visible })
}

/// True when `viewer` may see `entry`: membership first, then the audience
/// (RFC `authz.md` 5.5).
pub(super) fn entry_visible(campaign: &Campaign, entry: &FeedEntry, viewer: PlayerId) -> bool {
    if !member(campaign, viewer) {
        return false;
    }
    match entry.audience {
        Audience::Public => true,
        Audience::RollerAndDm => entry.actor == Some(viewer) || dm(campaign, viewer),
        Audience::RollerOnly => entry.actor == Some(viewer),
    }
}

#[cfg(test)]
mod tests {
    use localtt_core::types::ViewField;

    use super::ALL;

    /// The exhaustive match is the guard (OQ-3, OQ-8): adding a variant breaks
    /// this test's compilation, forcing a deliberate decision about the
    /// variant's `ALL` slot instead of silently dropping it from `hidden`, the
    /// body gate and the mask sweep. Mirrors `content::ContentKind::rank`.
    fn slot(field: ViewField) -> usize {
        match field {
            ViewField::Name => 0,
            ViewField::Identity => 1,
            ViewField::Abilities => 2,
            ViewField::Saves => 3,
            ViewField::Skills => 4,
            ViewField::Proficiencies => 5,
            ViewField::Hp => 6,
            ViewField::Ac => 7,
            ViewField::Speed => 8,
            ViewField::DeathSaves => 9,
            ViewField::Coins => 10,
            ViewField::Overrides => 11,
            ViewField::Resources => 12,
            ViewField::Spells => 13,
            ViewField::Inventory => 14,
            ViewField::Attacks => 15,
            ViewField::Features => 16,
            ViewField::Statuses => 17,
            ViewField::Notes => 18,
            ViewField::Ext => 19,
        }
    }

    #[test]
    fn all_lists_every_field_exactly_once() {
        let slots: Vec<usize> = ALL.iter().copied().map(slot).collect();

        assert_eq!(slots, (0..ALL.len()).collect::<Vec<_>>());
    }
}
