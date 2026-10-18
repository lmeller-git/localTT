//! Static tables: proficiency bonus, the 2014 multiclass and Pact Magic slot
//! tables, class hit-dice pools and the skill-to-ability map (RFC `rules.md`
//! 5.4, 5.9 and section 6).

use std::collections::BTreeMap;

use localtt_core::types::{
    Ability,
    ChallengeRating,
    EntityId,
    Profile,
    RefOrText,
    Skill,
    StatblockEntity,
};

use super::PactSlots;
use crate::error::{Warning, WarningCode};

/// The eighteen skills in canonical order.
pub(super) const SKILLS: [Skill; 18] = [
    Skill::Acrobatics,
    Skill::AnimalHandling,
    Skill::Arcana,
    Skill::Athletics,
    Skill::Deception,
    Skill::History,
    Skill::Insight,
    Skill::Intimidation,
    Skill::Investigation,
    Skill::Medicine,
    Skill::Nature,
    Skill::Perception,
    Skill::Performance,
    Skill::Persuasion,
    Skill::Religion,
    Skill::SleightOfHand,
    Skill::Stealth,
    Skill::Survival,
];

/// The ability a skill is based on (`rules.md` 5.4 structural bases).
pub(super) fn skill_ability(skill: Skill) -> Ability {
    match skill {
        Skill::Acrobatics | Skill::SleightOfHand | Skill::Stealth => Ability::Dexterity,
        Skill::AnimalHandling
        | Skill::Insight
        | Skill::Medicine
        | Skill::Perception
        | Skill::Survival => Ability::Wisdom,
        Skill::Arcana | Skill::History | Skill::Investigation | Skill::Nature | Skill::Religion => {
            Ability::Intelligence
        }
        Skill::Athletics => Ability::Strength,
        Skill::Deception | Skill::Intimidation | Skill::Performance | Skill::Persuasion => {
            Ability::Charisma
        }
    }
}

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

/// The static class tables' outcome for one entity.
pub(super) struct ClassTables {
    /// Regular spell slots by spell level, 1 to 9.
    pub(super) spell: BTreeMap<u8, i32>,
    /// Pact Magic, when the entity has warlock levels.
    pub(super) pact: Option<PactSlots>,
    /// Hit-dice pools by die sides.
    pub(super) hit_dice: BTreeMap<u8, i32>,
    /// Warnings for class names the tables do not model.
    pub(super) warnings: Vec<Warning>,
}

/// Applies the class tables to the entity's class levels (`rules.md` 5.9).
///
/// A class name outside [`CLASSES`] warns `UnmodeledClass` (element-less) and
/// contributes no slots and no hit dice; its levels still count toward the
/// total level and the proficiency bonus.
pub(super) fn class_tables(classes: &[(String, i32)], entity: EntityId) -> ClassTables {
    let mut full_caster_level: i32 = 0;
    let mut warlock_level: i32 = 0;
    let mut hit_dice: BTreeMap<u8, i32> = BTreeMap::new();
    let mut warnings = Vec::new();
    for (name, level) in classes {
        let Some(class) = class_table(name) else {
            warnings.push(Warning {
                code: WarningCode::UnmodeledClass,
                message: format!("`{name}` is not in the static class tables"),
                entity: Some(entity),
                element: None,
            });
            continue;
        };
        let dice = hit_dice.entry(class.hit_die).or_insert(0);
        *dice = dice.saturating_add(*level);
        match class.caster {
            Caster::Full => full_caster_level = full_caster_level.saturating_add(*level),
            Caster::Pact => warlock_level = warlock_level.saturating_add(*level),
            Caster::None => {}
        }
    }
    ClassTables {
        spell: spell_slots(full_caster_level),
        pact: pact_slots(warlock_level),
        hit_dice,
        warnings,
    }
}

/// The modeled classes: the four the golden catalog proves (`rules.md` OQ-2,
/// resolved).
const CLASSES: [ClassTable; 4] = [
    ClassTable {
        name: "Wizard",
        hit_die: 6,
        caster: Caster::Full,
    },
    ClassTable {
        name: "Cleric",
        hit_die: 8,
        caster: Caster::Full,
    },
    ClassTable {
        name: "Fighter",
        hit_die: 10,
        caster: Caster::None,
    },
    ClassTable {
        name: "Warlock",
        hit_die: 8,
        caster: Caster::Pact,
    },
];

/// One modeled class.
struct ClassTable {
    /// The class name snapshot, matched case-insensitively.
    name: &'static str,
    /// The hit-die size.
    hit_die: u8,
    /// How the class advances the slot tables.
    caster: Caster,
}

/// How a class advances the slot tables.
#[derive(Clone, Copy)]
enum Caster {
    /// A full caster: levels sum into the multiclass table.
    Full,
    /// Pact Magic: levels drive the separate warlock table.
    Pact,
    /// A non-caster.
    None,
}

/// The class-table entry whose name matches `name`, case-insensitively.
fn class_table(name: &str) -> Option<&'static ClassTable> {
    CLASSES
        .iter()
        .find(|class| name.eq_ignore_ascii_case(class.name))
}

/// The 2014 multiclass spellcaster table: slots per spell level (1st to 9th)
/// for combined full-caster levels 1 to 20.
const MULTICLASS_SLOTS: [[i32; 9]; 20] = [
    [2, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 0, 0, 0, 0, 0, 0, 0, 0],
    [4, 2, 0, 0, 0, 0, 0, 0, 0],
    [4, 3, 0, 0, 0, 0, 0, 0, 0],
    [4, 3, 2, 0, 0, 0, 0, 0, 0],
    [4, 3, 3, 0, 0, 0, 0, 0, 0],
    [4, 3, 3, 1, 0, 0, 0, 0, 0],
    [4, 3, 3, 2, 0, 0, 0, 0, 0],
    [4, 3, 3, 3, 1, 0, 0, 0, 0],
    [4, 3, 3, 3, 2, 0, 0, 0, 0],
    [4, 3, 3, 3, 2, 1, 0, 0, 0],
    [4, 3, 3, 3, 2, 1, 0, 0, 0],
    [4, 3, 3, 3, 2, 1, 1, 0, 0],
    [4, 3, 3, 3, 2, 1, 1, 0, 0],
    [4, 3, 3, 3, 2, 1, 1, 1, 0],
    [4, 3, 3, 3, 2, 1, 1, 1, 0],
    [4, 3, 3, 3, 2, 1, 1, 1, 1],
    [4, 3, 3, 3, 3, 1, 1, 1, 1],
    [4, 3, 3, 3, 3, 2, 1, 1, 1],
    [4, 3, 3, 3, 3, 2, 2, 1, 1],
];

/// The Pact Magic table: `(slot level, slot count)` by warlock level 1 to 20.
const PACT_SLOTS: [(u8, i32); 20] = [
    (1, 1),
    (1, 2),
    (2, 2),
    (2, 2),
    (3, 2),
    (3, 2),
    (4, 2),
    (4, 2),
    (5, 2),
    (5, 2),
    (5, 3),
    (5, 3),
    (5, 3),
    (5, 3),
    (5, 3),
    (5, 3),
    (5, 4),
    (5, 4),
    (5, 4),
    (5, 4),
];

/// The multiclass table row for a combined full-caster level, clamped to
/// 1..=20; no full-caster levels means no slots.
fn spell_slots(level: i32) -> BTreeMap<u8, i32> {
    if level <= 0 {
        return BTreeMap::new();
    }
    MULTICLASS_SLOTS[(level.clamp(1, 20) - 1) as usize]
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count > 0).then_some(((index + 1) as u8, *count)))
        .collect()
}

/// The Pact Magic row for a warlock level, clamped to 1..=20; no warlock
/// levels means no Pact Magic.
fn pact_slots(level: i32) -> Option<PactSlots> {
    if level <= 0 {
        return None;
    }
    let (level, count) = PACT_SLOTS[(level.clamp(1, 20) - 1) as usize];
    Some(PactSlots { level, count })
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

    #[test]
    fn the_multiclass_table_clamps_beyond_level_twenty() {
        assert_eq!(spell_slots(0), BTreeMap::new());
        assert_eq!(
            spell_slots(5),
            BTreeMap::from([(1, 4), (2, 3), (3, 2)]),
            "the golden multiclass row"
        );
        assert_eq!(spell_slots(1), BTreeMap::from([(1, 2)]));
        assert_eq!(
            spell_slots(20),
            BTreeMap::from([
                (1, 4),
                (2, 3),
                (3, 3),
                (4, 3),
                (5, 3),
                (6, 2),
                (7, 2),
                (8, 1),
                (9, 1)
            ])
        );
        assert_eq!(spell_slots(40), spell_slots(20));
    }

    #[test]
    fn the_pact_table_clamps_beyond_level_twenty() {
        assert_eq!(pact_slots(0), None);
        assert_eq!(pact_slots(2), Some(PactSlots { level: 1, count: 2 }));
        assert_eq!(pact_slots(11), Some(PactSlots { level: 5, count: 3 }));
        assert_eq!(pact_slots(40), pact_slots(20));
    }

    #[test]
    fn class_tables_sum_hit_dice_and_match_names_case_insensitively() {
        let classes = [
            ("wizard".to_owned(), 3),
            ("Wizard".to_owned(), 2),
            ("Fighter".to_owned(), 3),
            ("Warlock".to_owned(), 2),
        ];

        let tables = class_tables(&classes, EntityId::new());

        assert!(tables.warnings.is_empty());
        assert_eq!(tables.hit_dice, BTreeMap::from([(6, 5), (8, 2), (10, 3)]));
        assert_eq!(
            tables.spell,
            BTreeMap::from([(1, 4), (2, 3), (3, 2)]),
            "only the wizard levels are full-caster levels"
        );
        assert_eq!(tables.pact, Some(PactSlots { level: 1, count: 2 }));
    }

    #[test]
    fn unmodeled_classes_warn_and_contribute_nothing() {
        let classes = [("Bard".to_owned(), 3), ("Fighter".to_owned(), 1)];

        let tables = class_tables(&classes, EntityId::new());

        assert_eq!(tables.hit_dice, BTreeMap::from([(10, 1)]));
        assert_eq!(tables.spell, BTreeMap::new());
        assert_eq!(tables.pact, None);
        assert_eq!(tables.warnings.len(), 1);
        assert_eq!(tables.warnings[0].code, WarningCode::UnmodeledClass);
        assert_eq!(tables.warnings[0].element, None);
    }

    #[test]
    fn every_skill_maps_to_its_2014_ability() {
        let expected = [
            (Skill::Acrobatics, Ability::Dexterity),
            (Skill::AnimalHandling, Ability::Wisdom),
            (Skill::Arcana, Ability::Intelligence),
            (Skill::Athletics, Ability::Strength),
            (Skill::Deception, Ability::Charisma),
            (Skill::History, Ability::Intelligence),
            (Skill::Insight, Ability::Wisdom),
            (Skill::Intimidation, Ability::Charisma),
            (Skill::Investigation, Ability::Intelligence),
            (Skill::Medicine, Ability::Wisdom),
            (Skill::Nature, Ability::Intelligence),
            (Skill::Perception, Ability::Wisdom),
            (Skill::Performance, Ability::Charisma),
            (Skill::Persuasion, Ability::Charisma),
            (Skill::Religion, Ability::Intelligence),
            (Skill::SleightOfHand, Ability::Dexterity),
            (Skill::Stealth, Ability::Dexterity),
            (Skill::Survival, Ability::Wisdom),
        ];
        for (skill, ability) in expected {
            assert_eq!(skill_ability(skill), ability, "{skill:?}");
        }
        let mut listed = SKILLS;
        listed.sort();
        let mut expected_skills: Vec<Skill> = expected.iter().map(|(skill, _)| *skill).collect();
        expected_skills.sort();
        assert_eq!(
            listed.to_vec(),
            expected_skills,
            "SKILLS lists every skill once"
        );
    }
}
