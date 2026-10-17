//! U2 target test: engine content access over the shipped SRD snapshot, as a
//! client uses it.
//!
//! This file is the fixed point for `docs/rfcs/content.md`: client code written
//! against the pinned API before any library code exists. It intentionally does
//! not compile until the U2 skeleton lands (`localtt_engine::content` and
//! `localtt_adapter::reference` are not implemented yet); that is the spec-phase
//! contract, not an oversight. The shipped-data assertions also need the
//! committed `crates/localtt-adapter/data/srd2014/` snapshot, which the
//! implement phase generates with the converter.

use std::{collections::BTreeMap, fs, path::PathBuf};

use localtt_adapter::reference::{
    IndexedSources,
    SHIPPED_DATA_ROOT,
    SourceSnapshot,
    import::ImportReport,
    write_source,
};
use localtt_core::types::{Ext, ItemKind, Namespace, RefId};
use localtt_engine::content::{
    ContentData,
    ContentEntry,
    ContentFault,
    ContentKind,
    ContentSummary,
    Provenance,
    ReferenceSource,
    SearchQuery,
};
use tempfile::TempDir;

/// The pinned Open5e revision the shipped snapshot records.
const SRD_REVISION: &str = "b22294a4ebd8f5652135ad784eaa20070c4e62ff";

/// A reference id literal.
fn ref_id(raw: &str) -> RefId {
    RefId::parse(raw).expect("a literal reference is valid")
}

/// The shipped data root with the committed `srd2014` namespace.
fn shipped() -> IndexedSources {
    IndexedSources::from_dirs(&[PathBuf::from(SHIPPED_DATA_ROOT)])
        .expect("the shipped snapshot loads")
}

/// Ranked autocomplete over the shipped snapshot.
fn autocomplete(text: &str, kinds: &[ContentKind]) -> Vec<ContentSummary> {
    shipped()
        .autocomplete(&SearchQuery {
            text,
            kinds,
            limit: 100,
        })
        .expect("the shipped snapshot answers")
}

/// Provenance literals for hand-made test snapshots.
fn test_provenance() -> Provenance {
    Provenance {
        namespace: Namespace::parse("pack.test").expect("a literal namespace"),
        source: "Test pack".to_owned(),
        source_revision: "test-revision".to_owned(),
        license: "CC0".to_owned(),
        attribution: "Test fixture".to_owned(),
        url: None,
    }
}

#[test]
fn srd_spell_payload_is_typed() {
    let source = shipped();
    let entry = source
        .get(&ref_id("srd2014:spell/fireball"))
        .expect("a lookup")
        .expect("Fireball is in the SRD");

    assert_eq!(entry.kind, ContentKind::Spell);
    assert_eq!(entry.name, "Fireball");
    assert!(
        entry.description.contains("bright streak"),
        "the conversion keeps the spell text"
    );
    match &entry.data {
        ContentData::Spell(spell) => {
            assert_eq!(spell.level, 3);
            assert_eq!(spell.school, "Evocation");
            assert!(!spell.concentration);
            assert!(!spell.ritual);
        }
        other => panic!("a spell entry carries spell data, got {other:?}"),
    }
    assert_eq!(entry.provenance.namespace.as_str(), "srd2014");
    assert!(!entry.needs_review);
}

#[test]
fn srd_monster_payload_is_typed() {
    let source = shipped();
    let entry = source
        .get(&ref_id("srd2014:monster/goblin"))
        .expect("a lookup")
        .expect("Goblin is in the SRD");

    assert_eq!(entry.kind, ContentKind::Monster);
    assert_eq!(entry.name, "Goblin");
    match &entry.data {
        ContentData::Monster(monster) => {
            assert_eq!(monster.armor_class, 15);
            assert_eq!(monster.hit_points, 7);
            assert_eq!(monster.challenge_rating.as_eighths(), 2);
            assert_eq!(monster.abilities.strength, 8);
            assert_eq!(monster.abilities.dexterity, 14);
            assert_eq!(monster.abilities.constitution, 10);
            assert_eq!(monster.abilities.intelligence, 10);
            assert_eq!(monster.abilities.wisdom, 8);
            assert_eq!(monster.abilities.charisma, 8);
        }
        other => panic!("a monster entry carries monster data, got {other:?}"),
    }
}

#[test]
fn srd_item_payload_reuses_the_core_shape() {
    let source = shipped();
    let wand = source
        .get(&ref_id("srd2014:item/wand-of-fireballs"))
        .expect("a lookup")
        .expect("Wand of Fireballs is in the SRD");

    assert_eq!(wand.kind, ContentKind::Item);
    match &wand.data {
        ContentData::Item(item) => {
            assert_eq!(item.name, "Wand of Fireballs");
            assert!(item.requires_attunement, "the wand requires attunement");
            assert_eq!(item.kind, ItemKind::Wondrous);
        }
        other => panic!("an item entry carries item data, got {other:?}"),
    }

    let bag = source
        .get(&ref_id("srd2014:item/bag-of-holding"))
        .expect("a lookup")
        .expect("Bag of Holding is in the SRD");
    match &bag.data {
        ContentData::Item(item) => {
            assert!(
                !item.requires_attunement,
                "a missing attunement key means no attunement"
            );
        }
        other => panic!("an item entry carries item data, got {other:?}"),
    }
}

#[test]
fn autocomplete_fireb_returns_fireball_first() {
    let hits = autocomplete("fireb", &[]);

    assert_eq!(
        hits.first().map(|hit| hit.id.as_str()),
        Some("srd2014:spell/fireball")
    );
    assert_eq!(hits.first().map(|hit| hit.name.as_str()), Some("Fireball"));
}

#[test]
fn autocomplete_ranks_name_prefixes_above_word_prefixes() {
    let hits = autocomplete("fire", &[]);
    let position = |id: &str| {
        hits.iter()
            .position(|hit| hit.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is among the fire hits"))
    };

    assert!(
        position("srd2014:spell/fireball") < position("srd2014:item/wand-of-fireballs"),
        "a name prefix outranks a word prefix"
    );
}

#[test]
fn kind_filter_order_decides_ties() {
    let spells_first = autocomplete("shield", &[ContentKind::Spell, ContentKind::Item]);
    assert_eq!(
        spells_first.first().map(|hit| hit.id.as_str()),
        Some("srd2014:spell/shield"),
        "with spells first, the spell wins the exact-name tie"
    );

    let items_first = autocomplete("shield", &[ContentKind::Item, ContentKind::Spell]);
    assert_eq!(
        items_first.first().map(|hit| hit.id.as_str()),
        Some("srd2014:item/shield"),
        "with items first, the item wins the exact-name tie"
    );
}

#[test]
fn aliases_resolve_one_hop() {
    let dir = TempDir::new().expect("a temp dir for the snapshot");
    let entries = vec![ContentEntry {
        id: ref_id("pack.test:item/crown-of-dusk"),
        kind: ContentKind::Item,
        name: "Crown of Dusk".to_owned(),
        description: "A test crown.".to_owned(),
        data: ContentData::Text,
        provenance: test_provenance(),
        needs_review: false,
        review_notes: Vec::new(),
        ext: Ext::default(),
    }];
    let snapshot = SourceSnapshot {
        provenance: test_provenance(),
        entries,
        aliases: vec![(
            ref_id("pack.test:item/crown-of-dusk-old"),
            ref_id("pack.test:item/crown-of-dusk"),
        )],
        report: ImportReport {
            format: "test".to_owned(),
            source_revision: "test-revision".to_owned(),
            counts: BTreeMap::new(),
            review: BTreeMap::new(),
        },
    };
    write_source(dir.path(), &snapshot).expect("the snapshot writes");

    let source =
        IndexedSources::from_dirs(&[dir.path().to_path_buf()]).expect("the snapshot loads");
    let entry = source
        .get(&ref_id("pack.test:item/crown-of-dusk-old"))
        .expect("a lookup")
        .expect("the alias resolves to its target");
    assert_eq!(entry.id.as_str(), "pack.test:item/crown-of-dusk");
}

#[test]
fn unknown_reference_is_none_not_a_fault() {
    let source = shipped();

    assert!(
        source
            .get(&ref_id("srd2014:spell/does-not-exist"))
            .expect("a lookup")
            .is_none()
    );
    // The golden cast references this background, but SRD 5.1 contains only
    // Acolyte: an unknown ref is an empty answer, not an error.
    assert!(
        source
            .get(&ref_id("srd2014:background/sage"))
            .expect("a lookup")
            .is_none()
    );
}

#[test]
fn sources_report_the_snapshot_provenance() {
    let source = shipped();
    let sources = source.sources();
    let srd = sources
        .iter()
        .find(|provenance| provenance.namespace.as_str() == "srd2014")
        .expect("the shipped namespace is loaded");

    assert_eq!(srd.license, "CC-BY-4.0");
    assert_eq!(srd.source_revision, SRD_REVISION);
    assert!(
        srd.attribution.contains("System Reference Document 5.1"),
        "the WotC attribution is surfaced verbatim"
    );
    assert_eq!(
        srd.url.as_deref(),
        Some("https://dnd.wizards.com/resources/systems-reference-document")
    );
}

#[test]
fn a_missing_root_is_unavailable() {
    let dir = TempDir::new().expect("a temp dir");

    match IndexedSources::from_dirs(&[dir.path().join("missing")]) {
        Err(ContentFault::Unavailable { .. }) => {}
        Err(other) => panic!("a missing root must be unavailable, got {other:?}"),
        Ok(_) => panic!("a missing root must not load"),
    }
}

#[test]
fn a_corrupt_index_is_refused() {
    let dir = TempDir::new().expect("a temp dir");
    let namespace = dir.path().join("srd2014");
    fs::create_dir_all(&namespace).expect("the namespace dir");
    fs::write(namespace.join("index.json"), b"{ this is not json").expect("the broken index");

    match IndexedSources::from_dirs(&[dir.path().to_path_buf()]) {
        Err(ContentFault::Corrupt { .. }) => {}
        Err(other) => panic!("a malformed index must be corrupt, got {other:?}"),
        Ok(_) => panic!("a malformed index must not load"),
    }
}
