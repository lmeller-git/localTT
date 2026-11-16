//! The in-memory fixture source (`docs/architecture.md` section 8).
//!
//! The fixture is the "every port ships an in-memory implementation" side of
//! U2: a fixed catalog covering every `ContentKind`, duplicate names across
//! kinds, a unicode name and one `needs_review` entry. It composes the same
//! index as the directory source, so ranking, aliasing and validation have one
//! implementation.

use localtt_core::types::{
    Abilities,
    ChallengeRating,
    Coins,
    DamageType,
    DiceExpr,
    Ext,
    FeatureCategory,
    InlineItem,
    ItemKind,
    ItemProperties,
    Namespace,
    RefId,
    WeaponProperties,
    WeaponTag,
};
use localtt_engine::content::{
    ContentData,
    ContentEntry,
    ContentFault,
    ContentKind,
    ContentSummary,
    FeatureData,
    MonsterData,
    Provenance,
    ReferenceSource,
    SearchQuery,
    SpellData,
};

use crate::reference::{index::IndexedSources, source::SnapshotReader};

/// An in-memory [`ReferenceSource`] over the fixed fixture catalog.
pub struct FixtureSource {
    /// The shared index over the fixture snapshot.
    index: IndexedSources,
}

impl FixtureSource {
    /// The fixture catalog as a source.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::compose(Vec::new()).expect("the fixture catalog is valid")
    }

    /// The fixture catalog plus an explicit alias table.
    ///
    /// An invalid table is refused as [`ContentFault::Corrupt`]: an alias
    /// outside the fixture namespace, an alias that collides with a live id,
    /// a duplicate, a dead target or a chain.
    #[inline]
    pub fn with_aliases(aliases: Vec<(RefId, RefId)>) -> Result<Self, ContentFault> {
        Self::compose(aliases)
    }

    /// A copy of the fixture entries, for expectations in the contract suite.
    #[must_use]
    #[inline]
    pub fn catalog() -> Vec<ContentEntry> {
        catalog_entries()
    }

    /// Composes the catalog and the alias rows through the shared index.
    fn compose(aliases: Vec<(RefId, RefId)>) -> Result<Self, ContentFault> {
        let snapshot = MemorySnapshot::new(provenance(), catalog_entries(), aliases);
        let index = IndexedSources::from_readers(vec![Box::new(snapshot)])?;
        Ok(Self { index })
    }
}

impl Default for FixtureSource {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl ReferenceSource for FixtureSource {
    #[inline]
    fn get(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        self.index.get(id)
    }

    #[inline]
    fn search(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        self.index.search(query)
    }

    #[inline]
    fn autocomplete(&self, query: &SearchQuery<'_>) -> Result<Vec<ContentSummary>, ContentFault> {
        self.index.autocomplete(query)
    }

    #[inline]
    fn sources(&self) -> Vec<Provenance> {
        self.index.sources()
    }
}

/// The fixture's pinned provenance.
fn provenance() -> Provenance {
    Provenance {
        namespace: Namespace::parse("fixture").expect("a literal namespace"),
        source: "Fixture catalog".to_owned(),
        source_revision: "fixture-v1".to_owned(),
        license: "CC0".to_owned(),
        attribution: "localtt fixture data".to_owned(),
        url: None,
    }
}

/// One catalog entry under the fixture provenance.
fn entry(
    kind: ContentKind,
    slug: &str,
    name: &str,
    description: &str,
    data: ContentData,
) -> ContentEntry {
    ContentEntry {
        id: RefId::parse(&format!("fixture:{}/{slug}", kind.as_str()))
            .expect("a literal reference"),
        kind,
        name: name.to_owned(),
        description: description.to_owned(),
        data,
        provenance: provenance(),
        needs_review: false,
        review_notes: Vec::new(),
        ext: Ext::default(),
    }
}

/// The fixed fixture catalog: one entry per kind, a longsword with weapon
/// properties and a Thieves' Tools gear item, the `Shield` name shared by a
/// spell and an item, the unicode name `Élan`, a name with `fire` only as a
/// substring (`Bonfire`), a description-only `fire` match (`Acolyte`) and one
/// `needs_review` entry (`Cover`).
fn catalog_entries() -> Vec<ContentEntry> {
    let mut cover = entry(
        ContentKind::Rule,
        "cover",
        "Cover",
        "There are three degrees of cover: half, three-quarters and total.",
        ContentData::Text,
    );
    cover.needs_review = true;
    cover.review_notes = vec!["Fixture text: replace with verified source text.".to_owned()];

    vec![
        entry(
            ContentKind::Spell,
            "fire-bolt",
            "Fire Bolt",
            "A mote of fire streaks toward a creature in range.",
            ContentData::Spell(SpellData {
                level: 0,
                school: "Evocation".to_owned(),
                concentration: false,
                ritual: false,
            }),
        ),
        entry(
            ContentKind::Spell,
            "shield",
            "Shield",
            "A shimmering barrier of force protects you.",
            ContentData::Spell(SpellData {
                level: 1,
                school: "Abjuration".to_owned(),
                concentration: false,
                ritual: false,
            }),
        ),
        entry(
            ContentKind::Item,
            "shield",
            "Shield",
            "A shield is made from wood or metal.",
            ContentData::Item(InlineItem {
                name: "Shield".to_owned(),
                description: "A shield is made from wood or metal.".to_owned(),
                kind: ItemKind::Shield,
                requires_attunement: false,
                weight_oz: Some(96),
                value: None,
                properties: None,
                modifiers: Vec::new(),
                resource: None,
                ext: Ext::default(),
            }),
        ),
        entry(
            ContentKind::Item,
            "wand-of-fire",
            "Wand of Fire",
            "A wand crackling with restrained flame.",
            ContentData::Item(InlineItem {
                name: "Wand of Fire".to_owned(),
                description: "A wand crackling with restrained flame.".to_owned(),
                kind: ItemKind::Wondrous,
                requires_attunement: true,
                weight_oz: Some(16),
                value: None,
                properties: None,
                modifiers: Vec::new(),
                resource: None,
                ext: Ext::default(),
            }),
        ),
        entry(
            ContentKind::Item,
            "longsword",
            "Longsword",
            "A martial melee weapon with a long blade.",
            ContentData::Item(InlineItem {
                name: "Longsword".to_owned(),
                description: "A martial melee weapon with a long blade.".to_owned(),
                kind: ItemKind::Weapon,
                requires_attunement: false,
                weight_oz: Some(48),
                value: Some(Coins {
                    gp: 15,
                    ..Coins::default()
                }),
                properties: Some(ItemProperties::Weapon(WeaponProperties {
                    damage: Some(DiceExpr::Roll {
                        count: 1,
                        sides: 8,
                        keep: None,
                    }),
                    damage_type: Some(DamageType::Slashing),
                    range_ft: None,
                    versatile_damage: Some(DiceExpr::Roll {
                        count: 1,
                        sides: 10,
                        keep: None,
                    }),
                    tags: vec![WeaponTag::Versatile],
                })),
                modifiers: Vec::new(),
                resource: None,
                ext: Ext::default(),
            }),
        ),
        entry(
            ContentKind::Item,
            "thieves-tools",
            "Thieves' Tools",
            "Tools for picking locks and disarming traps.",
            ContentData::Item(InlineItem {
                name: "Thieves' Tools".to_owned(),
                description: "Tools for picking locks and disarming traps.".to_owned(),
                kind: ItemKind::Gear,
                requires_attunement: false,
                weight_oz: Some(16),
                value: Some(Coins {
                    gp: 25,
                    ..Coins::default()
                }),
                properties: None,
                modifiers: Vec::new(),
                resource: None,
                ext: Ext::default(),
            }),
        ),
        entry(
            ContentKind::Monster,
            "goblin",
            "Goblin",
            "A small, black-hearted humanoid that lairs in caves.",
            ContentData::Monster(MonsterData {
                abilities: Abilities {
                    strength: 8,
                    dexterity: 14,
                    constitution: 10,
                    intelligence: 10,
                    wisdom: 8,
                    charisma: 8,
                },
                challenge_rating: ChallengeRating::parse("1/4")
                    .expect("a literal challenge rating"),
                armor_class: 15,
                hit_points: 7,
            }),
        ),
        entry(
            ContentKind::Condition,
            "prone",
            "Prone",
            "A prone creature's only movement option is to crawl.",
            ContentData::Text,
        ),
        entry(
            ContentKind::Class,
            "fighter",
            "Fighter",
            "A master of martial combat.",
            ContentData::Text,
        ),
        entry(
            ContentKind::Subclass,
            "champion",
            "Champion",
            "The archetypal Champion focuses on raw physical power.",
            ContentData::Text,
        ),
        entry(
            ContentKind::Race,
            "elf",
            "Elf",
            "Elves are a magical people of otherworldly grace.",
            ContentData::Text,
        ),
        entry(
            ContentKind::Background,
            "acolyte",
            "Acolyte",
            "You have spent your life in the service of a temple, tending its sacred fire.",
            ContentData::Text,
        ),
        entry(
            ContentKind::Feat,
            "elan",
            "Élan",
            "You channel a sudden surge of vitality.",
            ContentData::Text,
        ),
        entry(
            ContentKind::Feature,
            "second-wind",
            "Second Wind",
            "You regain hit points equal to 1d10 + your fighter level.",
            ContentData::Feature(FeatureData {
                category: FeatureCategory::Class,
                text: "You regain hit points equal to 1d10 + your fighter level.".to_owned(),
            }),
        ),
        entry(
            ContentKind::Rule,
            "bonfire",
            "Bonfire",
            "A bonfire's flames light the surrounding area.",
            ContentData::Text,
        ),
        cover,
    ]
}

/// An in-memory snapshot over explicit entries (crate-private).
pub(crate) struct MemorySnapshot {
    /// Namespace, licensing and pinned revision.
    provenance: Provenance,
    /// The entries, as given.
    entries: Vec<ContentEntry>,
    /// `(alias, target)` rows, as given.
    aliases: Vec<(RefId, RefId)>,
}

impl MemorySnapshot {
    /// Builds a snapshot over `entries` and `aliases`.
    #[inline]
    pub(crate) fn new(
        provenance: Provenance,
        entries: Vec<ContentEntry>,
        aliases: Vec<(RefId, RefId)>,
    ) -> Self {
        Self {
            provenance,
            entries,
            aliases,
        }
    }
}

impl SnapshotReader for MemorySnapshot {
    #[inline]
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    #[inline]
    fn summaries(&self) -> Result<Vec<ContentSummary>, ContentFault> {
        Ok(self
            .entries
            .iter()
            .map(|entry| ContentSummary {
                id: entry.id.clone(),
                kind: entry.kind,
                name: entry.name.clone(),
            })
            .collect())
    }

    #[inline]
    fn entry(&self, id: &RefId) -> Result<Option<ContentEntry>, ContentFault> {
        Ok(self.entries.iter().find(|entry| entry.id == *id).cloned())
    }

    #[inline]
    fn aliases(&self) -> Result<Vec<(RefId, RefId)>, ContentFault> {
        Ok(self.aliases.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use localtt_core::types::{
        DamageType,
        DiceExpr,
        Ext,
        ItemKind,
        ItemProperties,
        RefId,
        WeaponTag,
    };
    use localtt_engine::content::{
        ContentData,
        ContentEntry,
        ContentFault,
        ContentKind,
        ReferenceSource,
        SearchQuery,
    };

    use super::{FixtureSource, MemorySnapshot, provenance};
    use crate::reference::source::SnapshotReader;

    fn ref_id(raw: &str) -> RefId {
        RefId::parse(raw).expect("a literal reference")
    }

    fn entry(slug: &str, name: &str) -> ContentEntry {
        ContentEntry {
            id: ref_id(&format!("fixture:spell/{slug}")),
            kind: ContentKind::Spell,
            name: name.to_owned(),
            description: "A test spell.".to_owned(),
            data: ContentData::Text,
            provenance: provenance(),
            needs_review: false,
            review_notes: Vec::new(),
            ext: Ext::default(),
        }
    }

    #[test]
    fn summaries_mirror_the_entries() {
        let snapshot = MemorySnapshot::new(
            provenance(),
            vec![entry("fire-bolt", "Fire Bolt"), entry("shield", "Shield")],
            Vec::new(),
        );

        let summaries = snapshot.summaries().expect("summaries");
        let ids: Vec<&str> = summaries.iter().map(|hit| hit.id.as_str()).collect();

        assert_eq!(ids, ["fixture:spell/fire-bolt", "fixture:spell/shield"]);
        assert_eq!(summaries[1].name, "Shield");
    }

    #[test]
    fn entry_finds_live_ids_and_misses_unknown_ones() {
        let snapshot = MemorySnapshot::new(
            provenance(),
            vec![entry("fire-bolt", "Fire Bolt")],
            Vec::new(),
        );

        assert_eq!(
            snapshot
                .entry(&ref_id("fixture:spell/fire-bolt"))
                .expect("a lookup"),
            Some(entry("fire-bolt", "Fire Bolt"))
        );
        assert_eq!(
            snapshot
                .entry(&ref_id("fixture:spell/missing"))
                .expect("a lookup"),
            None
        );
    }

    #[test]
    fn aliases_are_returned_verbatim() {
        let alias = ref_id("fixture:spell/fire-bolt-old");
        let target = ref_id("fixture:spell/fire-bolt");
        let snapshot = MemorySnapshot::new(
            provenance(),
            vec![entry("fire-bolt", "Fire Bolt")],
            vec![(alias.clone(), target.clone())],
        );

        assert_eq!(snapshot.aliases().expect("aliases"), [(alias, target)]);
        assert_eq!(snapshot.provenance().namespace.as_str(), "fixture");
    }

    #[test]
    fn catalog_covers_every_kind() {
        let kinds: BTreeSet<ContentKind> = FixtureSource::catalog()
            .iter()
            .map(|entry| entry.kind)
            .collect();

        assert_eq!(kinds, BTreeSet::from(ContentKind::ALL));
    }

    #[test]
    fn catalog_pins_duplicate_names_unicode_and_review() {
        let catalog = FixtureSource::catalog();

        let shields: Vec<&ContentEntry> = catalog
            .iter()
            .filter(|entry| entry.name == "Shield")
            .collect();
        assert_eq!(shields.len(), 2, "Shield must be duplicated across kinds");
        assert_ne!(shields[0].kind, shields[1].kind);
        assert!(catalog.iter().any(|entry| entry.name == "Élan"));

        let reviewed: Vec<&ContentEntry> =
            catalog.iter().filter(|entry| entry.needs_review).collect();
        assert_eq!(reviewed.len(), 1);
        assert!(!reviewed[0].review_notes.is_empty());
    }

    #[test]
    fn new_source_serves_the_catalog() {
        let source = FixtureSource::new();

        assert_eq!(
            source
                .get(&ref_id("fixture:spell/fire-bolt"))
                .expect("a lookup")
                .expect("reachable")
                .name,
            "Fire Bolt"
        );
        assert!(
            source
                .get(&ref_id("fixture:spell/missing"))
                .expect("a lookup")
                .is_none()
        );
        assert_eq!(source.sources(), vec![provenance()]);
        assert_eq!(FixtureSource::default().sources(), source.sources());
    }

    #[test]
    fn catalog_includes_a_weapon_and_a_gear_item() {
        let catalog = FixtureSource::catalog();
        let sword = catalog
            .iter()
            .find(|entry| entry.id == ref_id("fixture:item/longsword"))
            .expect("the fixture longsword");
        match &sword.data {
            ContentData::Item(item) => {
                assert_eq!(item.kind, ItemKind::Weapon);
                match &item.properties {
                    Some(ItemProperties::Weapon(weapon)) => {
                        assert_eq!(
                            weapon.damage,
                            Some(DiceExpr::Roll {
                                count: 1,
                                sides: 8,
                                keep: None,
                            })
                        );
                        assert_eq!(weapon.damage_type, Some(DamageType::Slashing));
                        assert_eq!(
                            weapon.versatile_damage,
                            Some(DiceExpr::Roll {
                                count: 1,
                                sides: 10,
                                keep: None,
                            })
                        );
                        assert_eq!(weapon.tags, vec![WeaponTag::Versatile]);
                    }
                    other => panic!("the longsword carries weapon properties, got {other:?}"),
                }
            }
            other => panic!("the longsword is an item, got {other:?}"),
        }

        let tools = catalog
            .iter()
            .find(|entry| entry.id == ref_id("fixture:item/thieves-tools"))
            .expect("the fixture gear item");
        match &tools.data {
            ContentData::Item(item) => {
                assert_eq!(item.kind, ItemKind::Gear);
                assert!(item.properties.is_none());
            }
            other => panic!("Thieves' Tools is an item, got {other:?}"),
        }
    }

    #[test]
    fn with_aliases_resolves_valid_rows_and_refuses_invalid_ones() {
        let alias = ref_id("fixture:spell/pyrotechnics");
        let valid =
            FixtureSource::with_aliases(vec![(alias.clone(), ref_id("fixture:spell/fire-bolt"))])
                .expect("a valid alias table");
        assert_eq!(
            valid
                .get(&alias)
                .expect("a lookup")
                .expect("the alias resolves")
                .id,
            ref_id("fixture:spell/fire-bolt")
        );
        assert!(
            valid
                .autocomplete(&SearchQuery {
                    text: "pyrotechnics",
                    kinds: &[],
                    limit: 10,
                })
                .expect("answers")
                .is_empty(),
            "alias ids are never summarized"
        );

        let cases: [(&str, Vec<(RefId, RefId)>); 6] = [
            (
                "an alias colliding with a live id",
                vec![(
                    ref_id("fixture:spell/fire-bolt"),
                    ref_id("fixture:spell/shield"),
                )],
            ),
            (
                "a duplicate alias",
                vec![
                    (alias.clone(), ref_id("fixture:spell/fire-bolt")),
                    (alias.clone(), ref_id("fixture:spell/shield")),
                ],
            ),
            (
                "a dead target",
                vec![(alias.clone(), ref_id("fixture:spell/missing"))],
            ),
            ("a self alias", vec![(alias.clone(), alias.clone())]),
            (
                "a chain",
                vec![
                    (alias.clone(), ref_id("fixture:spell/fire-bolt-old")),
                    (
                        ref_id("fixture:spell/fire-bolt-old"),
                        ref_id("fixture:spell/fire-bolt"),
                    ),
                ],
            ),
            (
                "a foreign namespace",
                vec![(
                    ref_id("pack.other:spell/pyrotechnics"),
                    ref_id("fixture:spell/fire-bolt"),
                )],
            ),
        ];
        for (label, rows) in cases {
            assert!(
                matches!(
                    FixtureSource::with_aliases(rows),
                    Err(ContentFault::Corrupt { .. })
                ),
                "{label} must be refused"
            );
        }
    }
}
