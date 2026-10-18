//! Class tables: proficiency bonus, multiclass spell slots, Pact Magic and
//! class hit-dice pools (RFC `rules.md` 5.9 and section 6).

use localtt_core::types::{ChallengeRating, Profile, RefOrText, StatblockEntity};

/// Class name snapshots with their levels, in document order.
pub(super) fn class_snapshots(entity: &StatblockEntity) -> Vec<(String, i32)> {
    entity
        .body
        .levels
        .iter()
        .map(|level| (class_name(&level.class), level.level))
        .collect()
}

/// Total character level: the saturating sum of class levels.
pub(super) fn total_level(classes: &[(String, i32)]) -> i32 {
    classes
        .iter()
        .fold(0, |total, &(_, level)| total.saturating_add(level))
}

/// The entity's proficiency bonus (`rules.md` 5.4): characters by total
/// level, creatures by challenge rating, and 2 when the sheet has neither.
pub(super) fn proficiency_bonus(profile: Profile, cr: Option<ChallengeRating>, level: i32) -> i32 {
    match profile {
        Profile::Character => level_bonus(level),
        Profile::Creature => cr.map_or(2, |cr| cr_bonus(cr.as_eighths())),
    }
}

/// The class name snapshot: the reference's display name or the inline text.
fn class_name(class: &RefOrText) -> String {
    match class {
        RefOrText::Ref(reference) => reference.name.clone(),
        RefOrText::Text(text) => text.clone(),
    }
}

/// The 2014 character table, clamped to levels 1..=20.
fn level_bonus(level: i32) -> i32 {
    2 + (level.clamp(1, 20) - 1) / 4
}

/// The 2014 monster table by whole challenge rating (fractions under 1 share
/// the CR 0 band). `eighths` is a valid rating in eighths.
fn cr_bonus(eighths: i32) -> i32 {
    2 + (eighths / 8 - 1).max(0) / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_proficiency_follows_the_2014_bands() {
        for (level, expected) in [
            (0, 2),
            (1, 2),
            (4, 2),
            (5, 3),
            (8, 3),
            (9, 4),
            (13, 5),
            (17, 6),
            (20, 6),
            (21, 6),
        ] {
            assert_eq!(
                proficiency_bonus(Profile::Character, None, level),
                expected,
                "level {level}"
            );
        }
    }

    #[test]
    fn creature_proficiency_follows_the_challenge_rating() {
        for (eighths, expected) in [(0, 2), (1, 2), (4, 2), (32, 2), (40, 3), (240, 9)] {
            let cr = ChallengeRating::from_eighths(eighths).expect("a valid rating");
            assert_eq!(
                proficiency_bonus(Profile::Creature, Some(cr), 0),
                expected,
                "CR in eighths {eighths}"
            );
        }
        assert_eq!(proficiency_bonus(Profile::Creature, None, 5), 2);
    }
}
