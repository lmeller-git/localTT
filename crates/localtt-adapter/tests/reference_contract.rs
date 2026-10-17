//! The content contract suite: one shared set of checks, run against every
//! `ReferenceSource` implementation (`docs/rfcs/content.md` section 6.6).
//!
//! The two implementations U2 ships are the in-memory `FixtureSource` and the
//! directory-backed `IndexedSources`; the second test also drives raw files
//! under a tempdir through `write_source` (missing files, malformed content,
//! version refusal, duplicate namespaces). Fixtures are adapter-local, like
//! `store_contract`: the golden cast stays in `localtt-integration`.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use localtt_adapter::reference::{
    CURRENT_SNAPSHOT_VERSION,
    FixtureSource,
    IndexedSources,
    SourceSnapshot,
    import::ImportReport,
    write_source,
};
use localtt_core::types::{Ext, Namespace, RefId};
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
use serde_json::{Value, json};
use tempfile::TempDir;

/// Supplies the shared suite with the sources it drives.
///
/// The harness owns implementation details (temp dirs, snapshot files) so the
/// suite itself only ever sees the [`ReferenceSource`] port.
trait ReferenceHarness {
    /// Returns a fresh source with the fixture catalog; every call is
    /// independent of earlier ones.
    fn fresh(&mut self) -> Box<dyn ReferenceSource>;
}

/// Runs the shared contract suite against `harness`'s implementation.
fn reference_contract(harness: &mut dyn ReferenceHarness) {
    let source = harness.fresh();
    let port: &dyn ReferenceSource = source.as_ref();
    let catalog = FixtureSource::catalog();

    // `get`: every catalog entry is reachable with an equal body.
    for expected in &catalog {
        let found = port
            .get(&expected.id)
            .expect("a lookup")
            .unwrap_or_else(|| panic!("{} must be reachable", expected.id));
        assert_eq!(found, *expected, "{} must round-trip", expected.id);
    }

    // Unknown ids and unknown namespaces answer `Ok(None)`, never an error.
    assert!(
        port.get(&ref_id("fixture:spell/missing"))
            .expect("a lookup")
            .is_none()
    );
    assert!(
        port.get(&ref_id("pack.other:spell/fire-bolt"))
            .expect("a lookup")
            .is_none()
    );

    // One provenance per loaded namespace, in load order.
    let provenance = catalog
        .first()
        .expect("a non-empty catalog")
        .provenance
        .clone();
    assert_eq!(port.sources(), vec![provenance]);

    // Ranking: exact > name prefix > word prefix > substring > description;
    // `autocomplete` never scans descriptions.
    assert_eq!(
        autocomplete_ids(port, "fire", &[]),
        [
            "fixture:spell/fire-bolt",
            "fixture:item/wand-of-fire",
            "fixture:rule/bonfire",
        ]
    );
    assert_eq!(
        search_ids(port, "fire", &[], 100),
        [
            "fixture:spell/fire-bolt",
            "fixture:item/wand-of-fire",
            "fixture:rule/bonfire",
            "fixture:background/acolyte",
        ]
    );
    assert_eq!(
        autocomplete_ids(port, "bonfire", &[]),
        ["fixture:rule/bonfire"]
    );
    assert!(autocomplete_ids(port, "sacred", &[]).is_empty());
    assert_eq!(
        search_ids(port, "sacred", &[], 100),
        ["fixture:background/acolyte"]
    );

    // The kind-filter order decides exact-name ties; unfiltered follows
    // `ContentKind::ALL`.
    assert_eq!(
        autocomplete_ids(port, "shield", &[ContentKind::Item, ContentKind::Spell]),
        ["fixture:item/shield", "fixture:spell/shield"]
    );
    assert_eq!(
        autocomplete_ids(port, "shield", &[ContentKind::Spell, ContentKind::Item]),
        ["fixture:spell/shield", "fixture:item/shield"]
    );
    assert_eq!(
        autocomplete_ids(port, "shield", &[]),
        ["fixture:spell/shield", "fixture:item/shield"]
    );

    // Empty text yields nothing; a zero limit yields nothing; the limit
    // truncates after ranking; reruns are stable.
    assert!(autocomplete_ids(port, "", &[]).is_empty());
    assert!(search_ids(port, "   ", &[], 100).is_empty());
    assert!(search_ids(port, "fire", &[], 0).is_empty());
    assert_eq!(
        search_ids(port, "fire", &[], 2),
        ["fixture:spell/fire-bolt", "fixture:item/wand-of-fire"]
    );
    assert_eq!(
        search_ids(port, "fire", &[], 100),
        search_ids(port, "fire", &[], 100)
    );
}

#[test]
fn fixture_source_satisfies_the_contract() {
    reference_contract(&mut FixtureHarness);

    let alias = ref_id("fixture:item/shield-old");
    let target = ref_id("fixture:item/shield");
    let source = FixtureSource::with_aliases(vec![(alias.clone(), target.clone())])
        .expect("a valid alias table");

    assert_eq!(
        source
            .get(&alias)
            .expect("a lookup")
            .expect("the alias resolves")
            .id,
        target
    );
    assert!(
        autocomplete_ids(&source, "shield-old", &[]).is_empty(),
        "alias ids are never summarized"
    );
    assert!(
        search_ids(&source, "shield", &[], 100)
            .iter()
            .all(|id| id.as_str() != alias.as_str()),
        "alias ids never appear in results"
    );

    let invalid: [(&str, Vec<(RefId, RefId)>); 6] = [
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
    for (label, rows) in invalid {
        assert!(
            matches!(
                FixtureSource::with_aliases(rows),
                Err(ContentFault::Corrupt { .. })
            ),
            "{label} must be refused"
        );
    }
}

#[test]
fn indexed_sources_satisfy_the_contract() {
    let mut harness = IndexedHarness::new();
    reference_contract(&mut harness);

    let root = harness.root.path().to_path_buf();

    // Determinism: rewriting the same snapshot reproduces every byte, and
    // entries.jsonl is sorted by id.
    let before = namespace_files(&root);
    write_source(&root, &fixture_snapshot(Vec::new())).expect("the snapshot rewrites");
    assert_eq!(
        before,
        namespace_files(&root),
        "a rerun must be byte-identical"
    );
    let lines = fs::read_to_string(root.join("fixture").join("entries.jsonl")).expect("entries");
    let ids: Vec<String> = lines
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).expect("a line");
            record["id"].as_str().expect("an id").to_owned()
        })
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "entries.jsonl is sorted by id");

    // A valid alias table resolves one hop, alias ids never appear in
    // results, and emptying the table removes the stale file.
    let alias = ref_id("fixture:item/shield-old");
    let target = ref_id("fixture:item/shield");
    write_source(
        &root,
        &fixture_snapshot(vec![(alias.clone(), target.clone())]),
    )
    .expect("the aliased snapshot writes");
    assert!(root.join("fixture").join("aliases.json").exists());
    let aliased =
        IndexedSources::from_dirs(std::slice::from_ref(&root)).expect("the aliased snapshot loads");
    assert_eq!(
        aliased
            .get(&alias)
            .expect("a lookup")
            .expect("the alias resolves")
            .id,
        target
    );
    assert!(
        search_ids(&aliased, "shield-old", &[], 100).is_empty(),
        "alias ids are never summarized"
    );
    assert!(
        search_ids(&aliased, "shield", &[], 100)
            .iter()
            .all(|id| id.as_str() != alias.as_str()),
        "alias ids never appear in results"
    );
    write_source(&root, &fixture_snapshot(Vec::new())).expect("the plain snapshot rewrites");
    assert!(
        !root.join("fixture").join("aliases.json").exists(),
        "a stale aliases.json must be removed"
    );

    // Strict loading: absent things are `Unavailable`.
    let empty = TempDir::new().expect("a temp dir");
    assert_unavailable(&[], "no configured roots");
    assert_unavailable(&[empty.path().join("missing")], "a missing root");
    assert_unavailable(&[empty.path().to_path_buf()], "a root without namespaces");

    let indexless = TempDir::new().expect("a temp dir");
    fs::create_dir_all(indexless.path().join("fixture")).expect("a namespace dir");
    fs::write(indexless.path().join("fixture").join("entries.jsonl"), b"").expect("entries");
    assert_unavailable(
        &[indexless.path().to_path_buf()],
        "a namespace without index.json",
    );

    let entryless = TempDir::new().expect("a temp dir");
    fs::create_dir_all(entryless.path().join("fixture")).expect("a namespace dir");
    fs::copy(
        root.join("fixture").join("index.json"),
        entryless.path().join("fixture").join("index.json"),
    )
    .expect("the index copies");
    assert_unavailable(
        &[entryless.path().to_path_buf()],
        "a namespace without entries.jsonl",
    );

    // Malformed content is `Corrupt`; the index is gated before
    // `entries.jsonl` is even checked.
    let broken_index = TempDir::new().expect("a temp dir");
    fs::create_dir_all(broken_index.path().join("fixture")).expect("a namespace dir");
    fs::write(
        broken_index.path().join("fixture").join("index.json"),
        b"{ not json",
    )
    .expect("a broken index");
    assert_corrupt(
        &[broken_index.path().to_path_buf()],
        "a malformed index (with entries.jsonl absent)",
    );

    let broken_aliases = TempDir::new().expect("a temp dir");
    write_source(
        broken_aliases.path(),
        &fixture_snapshot(vec![(alias.clone(), target.clone())]),
    )
    .expect("the aliased snapshot writes");
    fs::write(
        broken_aliases.path().join("fixture").join("aliases.json"),
        b"{ not json",
    )
    .expect("a broken alias file");
    assert_corrupt(
        &[broken_aliases.path().to_path_buf()],
        "a malformed alias file",
    );

    let renamed = TempDir::new().expect("a temp dir");
    write_source(renamed.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    fs::rename(
        renamed.path().join("fixture"),
        renamed.path().join("pack.fixture"),
    )
    .expect("the rename");
    assert_corrupt(
        &[renamed.path().to_path_buf()],
        "a directory name disagreeing with provenance",
    );

    // Backups of a namespace are pruned silently.
    let pruned = TempDir::new().expect("a temp dir");
    write_source(pruned.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    fs::rename(
        pruned.path().join("fixture"),
        pruned.path().join("fixture.bak"),
    )
    .expect("the rename");
    assert_unavailable(
        &[pruned.path().to_path_buf()],
        "a root holding only a pruned backup",
    );

    // Invalid directory names and plain files are pruned before any read.
    let stray = TempDir::new().expect("a temp dir");
    write_source(stray.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    fs::create_dir_all(stray.path().join("Not.A.Namespace")).expect("a stray dir");
    fs::write(
        stray.path().join("Not.A.Namespace").join("index.json"),
        b"{ not json",
    )
    .expect("stray contents");
    fs::write(stray.path().join("stray.json"), b"{ not json").expect("a stray file");
    let strays_pruned = load(&[stray.path().to_path_buf()]).expect("stray names are pruned");
    assert_eq!(strays_pruned.sources().len(), 1);

    // A version this build does not know is refused, not partially read.
    let newer = TempDir::new().expect("a temp dir");
    write_source(newer.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    let index_path = newer.path().join("fixture").join("index.json");
    let mut index: Value =
        serde_json::from_slice(&fs::read(&index_path).expect("index bytes")).expect("an index");
    index["snapshot_version"] = json!(CURRENT_SNAPSHOT_VERSION + 1);
    fs::write(&index_path, serde_json::to_vec(&index).expect("json")).expect("the newer index");
    match load(&[newer.path().to_path_buf()]) {
        Err(ContentFault::SnapshotVersionRefused { found, supported }) => {
            assert_eq!(found, CURRENT_SNAPSHOT_VERSION + 1);
            assert_eq!(supported, CURRENT_SNAPSHOT_VERSION);
        }
        Err(other) => panic!("a newer snapshot must be refused, got {other:?}"),
        Ok(_) => panic!("a newer snapshot must not load"),
    }

    // Two roots claiming the same namespace conflict.
    let first = TempDir::new().expect("a temp dir");
    let second = TempDir::new().expect("a temp dir");
    write_source(first.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    write_source(second.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    match load(&[first.path().to_path_buf(), second.path().to_path_buf()]) {
        Err(ContentFault::NamespaceConflict { namespace }) => {
            assert_eq!(namespace.as_str(), "fixture");
        }
        Err(other) => panic!("a duplicate namespace must conflict, got {other:?}"),
        Ok(_) => panic!("a duplicate namespace must not load"),
    }

    // Namespaces order ascending; the kind priority outranks them.
    let both = TempDir::new().expect("a temp dir");
    write_source(both.path(), &fixture_snapshot(Vec::new())).expect("the fixture writes");
    write_source(both.path(), &other_namespace_snapshot()).expect("the pack writes");
    let loaded = load(&[both.path().to_path_buf()]).expect("both namespaces load");
    let sources = loaded.sources();
    let namespaces: Vec<&str> = sources
        .iter()
        .map(|provenance| provenance.namespace.as_str())
        .collect();
    assert_eq!(
        namespaces,
        ["fixture", "pack.other"],
        "load order: roots, then ascending directory names"
    );
    assert_eq!(
        search_ids(&loaded, "shield", &[], 100),
        [
            "fixture:spell/shield",
            "pack.other:spell/shield",
            "fixture:item/shield",
        ],
        "kind priority outranks namespace in the sort key"
    );

    // Bodies stay lazy: a malformed body reports on the read, not on open.
    let broken_body = TempDir::new().expect("a temp dir");
    write_source(broken_body.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    fs::write(
        broken_body.path().join("fixture").join("entries.jsonl"),
        b"{ not json\n",
    )
    .expect("a broken body");
    let broken_body_source =
        load(&[broken_body.path().to_path_buf()]).expect("opening does not read bodies");
    match broken_body_source.get(&first_catalog_id()) {
        Err(ContentFault::Corrupt { .. }) => {}
        other => panic!("a malformed body must be corrupt on get, got {other:?}"),
    }
    match broken_body_source.search(&SearchQuery {
        text: "zzzz",
        kinds: &[],
        limit: 10,
    }) {
        Err(ContentFault::Corrupt { .. }) => {}
        other => panic!("a malformed body must be corrupt on search, got {other:?}"),
    }

    // A body disagreeing with its index row is corrupt on the read.
    let disagreeing = TempDir::new().expect("a temp dir");
    write_source(disagreeing.path(), &fixture_snapshot(Vec::new())).expect("the snapshot writes");
    let entries_path = disagreeing.path().join("fixture").join("entries.jsonl");
    let lines = fs::read_to_string(&entries_path).expect("entries");
    let first_line = lines.lines().next().expect("a first line");
    let mut record: Value = serde_json::from_str(first_line).expect("a line");
    record["name"] = json!("Renamed");
    let mut body = serde_json::to_vec(&record).expect("json");
    body.push(b'\n');
    fs::write(&entries_path, body).expect("a disagreeing body");
    let disagreeing_source =
        load(&[disagreeing.path().to_path_buf()]).expect("opening does not validate bodies");
    match disagreeing_source.get(&first_catalog_id()) {
        Err(ContentFault::Corrupt { .. }) => {}
        other => panic!("a body disagreeing with its row must be corrupt on get, got {other:?}"),
    }

    // Payload compatibility: a line missing later optional fields parses to
    // their defaults, and unknown fields are ignored.
    let compat = TempDir::new().expect("a temp dir");
    let lines = fs::read_to_string(root.join("fixture").join("entries.jsonl")).expect("entries");
    let line = lines
        .lines()
        .find(|line| line.contains("fixture:item/shield"))
        .expect("the shield item line");
    let mut record: Value = serde_json::from_str(line).expect("a line");
    let payload = record
        .get_mut("data")
        .and_then(Value::as_object_mut)
        .expect("an item payload");
    for key in ["weight_oz", "value", "properties", "resource"] {
        payload.remove(key);
    }
    record["future_field"] = json!(true);

    let compat_dir = compat.path().join("fixture");
    fs::create_dir_all(&compat_dir).expect("a namespace dir");
    let mut index: Value =
        serde_json::from_slice(&fs::read(root.join("fixture").join("index.json")).expect("index"))
            .expect("an index");
    index["rows"] = json!([{
        "id": "fixture:item/shield",
        "kind": "item",
        "name": "Shield",
        "offset": 0,
    }]);
    fs::write(
        compat_dir.join("index.json"),
        serde_json::to_vec(&index).expect("json"),
    )
    .expect("the index");
    let mut body = serde_json::to_vec(&record).expect("json");
    body.push(b'\n');
    fs::write(compat_dir.join("entries.jsonl"), body).expect("the entries");

    let compat_source =
        load(&[compat.path().to_path_buf()]).expect("the compatibility snapshot loads");
    let found = compat_source
        .get(&ref_id("fixture:item/shield"))
        .expect("a lookup")
        .expect("reachable");
    match &found.data {
        ContentData::Item(item) => {
            assert!(
                item.weight_oz.is_none(),
                "a missing optional field defaults"
            );
            assert!(item.value.is_none());
        }
        other => panic!("expected an item payload, got {other:?}"),
    }
}

/// The in-memory implementation.
struct FixtureHarness;

impl ReferenceHarness for FixtureHarness {
    fn fresh(&mut self) -> Box<dyn ReferenceSource> {
        Box::new(FixtureSource::new())
    }
}

/// The directory implementation over the fixture catalog in a tempdir.
struct IndexedHarness {
    /// The tempdir holding the `fixture/` namespace.
    root: TempDir,
}

impl IndexedHarness {
    /// Writes the fixture catalog into a fresh tempdir.
    fn new() -> Self {
        let root = TempDir::new().expect("a temp dir");
        write_source(root.path(), &fixture_snapshot(Vec::new()))
            .expect("the fixture snapshot writes");
        Self { root }
    }
}

impl ReferenceHarness for IndexedHarness {
    fn fresh(&mut self) -> Box<dyn ReferenceSource> {
        Box::new(
            IndexedSources::from_dirs(&[self.root.path().to_path_buf()])
                .expect("the fixture snapshot loads"),
        )
    }
}

fn ref_id(raw: &str) -> RefId {
    RefId::parse(raw).expect("a literal reference")
}

fn ids_of(hits: Vec<ContentSummary>) -> Vec<String> {
    hits.into_iter()
        .map(|hit| hit.id.as_str().to_owned())
        .collect()
}

fn autocomplete_ids(
    source: &dyn ReferenceSource,
    text: &str,
    kinds: &[ContentKind],
) -> Vec<String> {
    ids_of(
        source
            .autocomplete(&SearchQuery {
                text,
                kinds,
                limit: 100,
            })
            .expect("autocomplete answers"),
    )
}

fn search_ids(
    source: &dyn ReferenceSource,
    text: &str,
    kinds: &[ContentKind],
    limit: usize,
) -> Vec<String> {
    ids_of(
        source
            .search(&SearchQuery { text, kinds, limit })
            .expect("search answers"),
    )
}

fn first_catalog_id() -> RefId {
    let mut ids: Vec<RefId> = FixtureSource::catalog()
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    ids.sort();
    ids.into_iter().next().expect("a non-empty catalog")
}

/// Every file of the `fixture/` namespace, by name.
fn namespace_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root.join("fixture"))
        .expect("the namespace directory")
        .map(|entry| {
            let entry = entry.expect("a directory entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, fs::read(entry.path()).expect("a snapshot file"))
        })
        .collect()
}

fn load(roots: &[PathBuf]) -> Result<IndexedSources, ContentFault> {
    IndexedSources::from_dirs(roots)
}

fn assert_unavailable(roots: &[PathBuf], label: &str) {
    match load(roots) {
        Err(ContentFault::Unavailable { .. }) => {}
        Err(other) => panic!("{label} must be unavailable, got {other:?}"),
        Ok(_) => panic!("{label} must not load"),
    }
}

fn assert_corrupt(roots: &[PathBuf], label: &str) {
    match load(roots) {
        Err(ContentFault::Corrupt { .. }) => {}
        Err(other) => panic!("{label} must be corrupt, got {other:?}"),
        Ok(_) => panic!("{label} must not load"),
    }
}

/// The fixture catalog as a snapshot, with explicit alias rows.
fn fixture_snapshot(aliases: Vec<(RefId, RefId)>) -> SourceSnapshot {
    let provenance = fixture_provenance();
    SourceSnapshot {
        provenance: provenance.clone(),
        entries: FixtureSource::catalog(),
        aliases,
        report: ImportReport::new("fixture".to_owned(), provenance.source_revision.clone()),
    }
}

/// The fixture catalog's literal provenance.
fn fixture_provenance() -> Provenance {
    FixtureSource::new()
        .sources()
        .into_iter()
        .next()
        .expect("the fixture source has exactly one namespace")
}

/// A second namespace holding a `Shield` spell, to pin namespace ordering.
fn other_namespace_snapshot() -> SourceSnapshot {
    let provenance = Provenance {
        namespace: Namespace::parse("pack.other").expect("a literal namespace"),
        source: "Test pack".to_owned(),
        source_revision: "pack-v1".to_owned(),
        license: "CC0".to_owned(),
        attribution: "Test pack".to_owned(),
        url: None,
    };
    let entry = ContentEntry {
        id: ref_id("pack.other:spell/shield"),
        kind: ContentKind::Spell,
        name: "Shield".to_owned(),
        description: "A pack shield spell.".to_owned(),
        data: ContentData::Text,
        provenance: provenance.clone(),
        needs_review: false,
        review_notes: Vec::new(),
        ext: Ext::default(),
    };
    SourceSnapshot {
        provenance,
        entries: vec![entry],
        aliases: Vec::new(),
        report: ImportReport::new("pack".to_owned(), "pack-v1".to_owned()),
    }
}
