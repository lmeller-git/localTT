//! The pinned Open5e `wotc-srd` converter (`docs/rfcs/content.md` section 6.3).
//!
//! Reads the eleven `data/WOTC_5e_SRD_v5.1/*.json` datasets of
//! `open5e/open5e-api`, maps them to `ContentEntry`s and records the pinned
//! revision in the provenance and report. The tool does not read git: the
//! revision is given by the caller.

use std::{collections::BTreeMap, fs, path::Path};

use localtt_core::types::{
    Abilities,
    ChallengeRating,
    Coins,
    Ext,
    InlineItem,
    ItemKind,
    Namespace,
    RefId,
};
use localtt_engine::content::{
    ContentData,
    ContentEntry,
    ContentKind,
    MonsterData,
    Provenance,
    SpellData,
};
use serde_json::Value;

use crate::reference::{
    import::{ImportError, ImportReport, Importer},
    snapshot::SourceSnapshot,
};

/// The provenance `source` field pinned for the dataset.
const SOURCE: &str = "SRD 5.1 (CC-BY-4.0), via Open5e";
/// The pinned license identifier.
const LICENSE: &str = "CC-BY-4.0";
/// The pinned attribution text.
const ATTRIBUTION: &str = "This work includes material taken from the System Reference Document 5.1 (\"SRD 5.1\") by Wizards of the Coast LLC and available at https://dnd.wizards.com/resources/systems-reference-document. The SRD 5.1 is licensed under the Creative Commons Attribution 4.0 International License available at https://creativecommons.org/licenses/by/4.0/legalcode.";
/// The canonical source URL.
const URL: &str = "https://dnd.wizards.com/resources/systems-reference-document";
/// The per-entry ext slot: the source record verbatim.
const EXT_KEY: &str = "src.open5e";

/// The Open5e `wotc-srd` converter (SRD 5.1, 2014 rules).
#[derive(Clone, Debug)]
pub struct Open5eImporter {
    /// The namespace the snapshot is written under, e.g. `srd2014`.
    namespace: Namespace,
    /// The pinned upstream revision, recorded verbatim.
    source_revision: String,
}

impl Open5eImporter {
    /// Creates the converter for one namespace and revision.
    #[must_use]
    #[inline]
    pub fn new(namespace: Namespace, source_revision: String) -> Self {
        Self {
            namespace,
            source_revision,
        }
    }
}

impl Importer for Open5eImporter {
    #[inline]
    fn format(&self) -> &'static str {
        "open5e"
    }

    #[inline]
    fn import(&self, input: &Path) -> Result<SourceSnapshot, ImportError> {
        let mut catalog = Catalog::new(self.namespace.clone(), &self.source_revision);

        for record in &read_records(input, "spells")? {
            catalog.spell("spells.json", record)?;
        }
        for record in &read_records(input, "monsters")? {
            catalog.monster("monsters.json", record)?;
        }
        for record in &read_records(input, "magicitems")? {
            catalog.magic_item("magicitems.json", record)?;
        }
        for record in &read_records(input, "weapons")? {
            catalog.weapon("weapons.json", record)?;
        }
        for record in &read_records(input, "armor")? {
            catalog.armor("armor.json", record)?;
        }
        for record in &read_records(input, "conditions")? {
            catalog.text("conditions.json", ContentKind::Condition, record)?;
        }
        for record in &read_records(input, "classes")? {
            catalog.class("classes.json", record)?;
        }
        for record in &read_records(input, "races")? {
            catalog.race("races.json", record)?;
        }
        for record in &read_records(input, "backgrounds")? {
            catalog.text("backgrounds.json", ContentKind::Background, record)?;
        }
        for record in &read_records(input, "feats")? {
            catalog.feat("feats.json", record)?;
        }
        for record in &read_records(input, "sections")? {
            catalog.text("sections.json", ContentKind::Rule, record)?;
        }

        Ok(catalog.finish())
    }
}

/// Reads one dataset file as a top-level JSON array of records.
fn read_records(input: &Path, file: &str) -> Result<Vec<Value>, ImportError> {
    let name = format!("{file}.json");
    let path = input.join(&name);
    let bytes = fs::read(&path).map_err(|error| ImportError::Unavailable {
        message: format!("{}: {error}", path.display()),
    })?;
    serde_json::from_slice::<Vec<Value>>(&bytes).map_err(|error| ImportError::Corrupt {
        file: name,
        message: error.to_string(),
    })
}

/// One mapped record on its way into the catalog.
struct Mapped {
    /// Display name; the slug derives from it.
    name: String,
    /// Content kind; the id derives from it.
    kind: ContentKind,
    /// Entry description text.
    description: String,
    /// Structured payload.
    data: ContentData,
    /// Review notes; non-empty marks `needs_review`.
    notes: Vec<String>,
}

/// The translated fields of one item record, whatever its source file.
struct ItemParts {
    /// The mapped [`ItemKind`].
    kind: ItemKind,
    /// From the `requires-attunement` key.
    requires_attunement: bool,
    /// Parsed `weight`.
    weight_oz: Option<i32>,
    /// Parsed `cost`.
    value: Option<Coins>,
    /// Parse notes for the review map.
    notes: Vec<String>,
}

/// The mapped entries and their bookkeeping.
struct Catalog {
    /// Provenance recorded on every entry and in the snapshot.
    provenance: Provenance,
    /// The `src.open5e` ext key.
    ext_key: Namespace,
    /// Mapped entries, in source order.
    entries: Vec<ContentEntry>,
    /// `id -> "<file>: <name>"` of the first record that claimed it.
    claimed: BTreeMap<RefId, String>,
    /// Review notes per entry id.
    review: BTreeMap<RefId, Vec<String>>,
}

impl Catalog {
    /// Starts an empty catalog for one namespace and revision.
    fn new(namespace: Namespace, source_revision: &str) -> Self {
        Self {
            provenance: Provenance {
                namespace,
                source: SOURCE.to_owned(),
                source_revision: source_revision.to_owned(),
                license: LICENSE.to_owned(),
                attribution: ATTRIBUTION.to_owned(),
                url: Some(URL.to_owned()),
            },
            ext_key: Namespace::parse(EXT_KEY).expect("a literal namespace"),
            entries: Vec::new(),
            claimed: BTreeMap::new(),
            review: BTreeMap::new(),
        }
    }

    /// Maps one `spells.json` record.
    fn spell(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        let level = u8::try_from(integer(record, file, "level_int")?)
            .map_err(|_| corrupt(file, format!("spell {name} has a level outside 0..=255")))?;
        let school = text(record, file, "school")?;
        let concentration = text(record, file, "concentration")? == "yes";
        let ritual = text(record, file, "ritual")? == "yes";
        let casting_time = text(record, file, "casting_time")?;
        let range = text(record, file, "range")?;
        let components = text(record, file, "components")?;
        let material = optional_text(record, "material").filter(|material| !material.is_empty());
        let duration = text(record, file, "duration")?;
        let desc = text(record, file, "desc")?;
        let higher_level =
            optional_text(record, "higher_level").filter(|text| !text.trim().is_empty());

        let mut description = format!("*{} {school}*\n\n", level_label(level));
        description.push_str(&format!("**Casting Time:** {casting_time}\n"));
        description.push_str(&format!("**Range:** {range}\n"));
        match &material {
            Some(material) => {
                description.push_str(&format!("**Components:** {components} ({material})\n"));
            }
            None => description.push_str(&format!("**Components:** {components}\n")),
        }
        let duration = if concentration {
            format!("Concentration, {}", lowercase_first(&duration))
        } else {
            duration
        };
        description.push_str(&format!("**Duration:** {duration}\n\n"));
        description.push_str(desc.trim_end());
        if let Some(higher_level) = higher_level {
            description.push_str("\n\n**At Higher Levels.** ");
            description.push_str(higher_level.trim_end());
        }

        self.push(
            file,
            record,
            Mapped {
                name,
                kind: ContentKind::Spell,
                description,
                data: ContentData::Spell(SpellData {
                    level,
                    school,
                    concentration,
                    ritual,
                }),
                notes: Vec::new(),
            },
        )
    }

    /// Maps one `monsters.json` record.
    fn monster(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        let raw_rating = text(record, file, "challenge_rating")?;
        let challenge_rating = ChallengeRating::parse(&raw_rating).map_err(|_| {
            corrupt(
                file,
                format!("monster {name} has an invalid challenge rating {raw_rating:?}"),
            )
        })?;
        let abilities = Abilities {
            strength: int32(record, file, "strength")?,
            dexterity: int32(record, file, "dexterity")?,
            constitution: int32(record, file, "constitution")?,
            intelligence: int32(record, file, "intelligence")?,
            wisdom: int32(record, file, "wisdom")?,
            charisma: int32(record, file, "charisma")?,
        };
        let armor_class = int32(record, file, "armor_class")?;
        let hit_points = int32(record, file, "hit_points")?;

        let mut description = String::new();
        for key in [
            "special_abilities",
            "actions",
            "reactions",
            "legendary_actions",
        ] {
            let Some(section) = optional_array(record, file, key)? else {
                continue;
            };
            for ability in section {
                let ability_name = text(ability, file, "name")?;
                let ability_desc = text(ability, file, "desc")?;
                if !description.is_empty() {
                    description.push_str("\n\n");
                }
                description.push_str(&format!("**{ability_name}.** {}", ability_desc.trim_end()));
            }
        }

        self.push(
            file,
            record,
            Mapped {
                name,
                kind: ContentKind::Monster,
                description,
                data: ContentData::Monster(MonsterData {
                    abilities,
                    challenge_rating,
                    armor_class,
                    hit_points,
                }),
                notes: Vec::new(),
            },
        )
    }

    /// Maps one `magicitems.json` record.
    fn magic_item(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        let item_type = text(record, file, "type")?;
        let attunement = record.get("requires-attunement");
        let requires_attunement = attunement.is_some_and(|value| !value.is_null());

        let mut description = text(record, file, "desc")?.trim_end().to_owned();
        if let Some(qualifier) = attunement
            .and_then(Value::as_str)
            .filter(|qualifier| !qualifier.is_empty())
        {
            description.push_str("\n\n*");
            description.push_str(&capitalize_first(qualifier));
            description.push('*');
        }

        self.item(
            file,
            &name,
            description,
            ItemParts {
                kind: magic_item_kind(&item_type),
                requires_attunement,
                weight_oz: None,
                value: None,
                notes: Vec::new(),
            },
            record,
        )
    }

    /// Maps one `weapons.json` record.
    fn weapon(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        let measures = parse_measures(record);

        self.item(
            file,
            &name,
            String::new(),
            ItemParts {
                kind: ItemKind::Weapon,
                requires_attunement: false,
                weight_oz: measures.weight_oz,
                value: measures.value,
                notes: measures.notes,
            },
            record,
        )
    }

    /// Maps one `armor.json` record.
    fn armor(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        let kind = if name == "Shield" {
            ItemKind::Shield
        } else {
            ItemKind::Armor
        };
        let measures = parse_measures(record);

        self.item(
            file,
            &name,
            String::new(),
            ItemParts {
                kind,
                requires_attunement: false,
                weight_oz: measures.weight_oz,
                value: measures.value,
                notes: measures.notes,
            },
            record,
        )
    }

    /// Pushes one item entry; `description` is the entry text, the payload
    /// carries the same text.
    fn item(
        &mut self,
        file: &str,
        name: &str,
        description: String,
        parts: ItemParts,
        record: &Value,
    ) -> Result<(), ImportError> {
        self.push(
            file,
            record,
            Mapped {
                name: name.to_owned(),
                kind: ContentKind::Item,
                description: description.clone(),
                data: ContentData::Item(InlineItem {
                    name: name.to_owned(),
                    description,
                    kind: parts.kind,
                    requires_attunement: parts.requires_attunement,
                    weight_oz: parts.weight_oz,
                    value: parts.value,
                    properties: None,
                    modifiers: Vec::new(),
                    resource: None,
                    ext: Ext::default(),
                }),
                notes: parts.notes,
            },
        )
    }

    /// Maps one `classes.json` record: the class and its subclasses.
    fn class(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        let desc = field(record, file, "features")?
            .get("desc")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                corrupt(
                    file,
                    format!("class {name} features.desc is missing or not a string"),
                )
            })?;
        self.push(
            file,
            record,
            Mapped {
                name,
                kind: ContentKind::Class,
                description: desc.trim_end().to_owned(),
                data: ContentData::Text,
                notes: Vec::new(),
            },
        )?;

        for subtype in optional_array(record, file, "subtypes")?.unwrap_or_default() {
            self.push(
                file,
                subtype,
                Mapped {
                    name: text(subtype, file, "name")?,
                    kind: ContentKind::Subclass,
                    description: text(subtype, file, "desc")?,
                    data: ContentData::Text,
                    notes: Vec::new(),
                },
            )?;
        }
        Ok(())
    }

    /// Maps one `races.json` record: the race and its subraces.
    fn race(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        self.push(
            file,
            record,
            Mapped {
                name,
                kind: ContentKind::Race,
                description: combined_traits(
                    &text(record, file, "desc")?,
                    optional_text(record, "traits").as_deref(),
                ),
                data: ContentData::Text,
                notes: Vec::new(),
            },
        )?;

        for subtype in optional_array(record, file, "subtypes")?.unwrap_or_default() {
            self.push(
                file,
                subtype,
                Mapped {
                    name: text(subtype, file, "name")?,
                    kind: ContentKind::Race,
                    description: combined_traits(
                        &text(subtype, file, "desc")?,
                        optional_text(subtype, "traits").as_deref(),
                    ),
                    data: ContentData::Text,
                    notes: Vec::new(),
                },
            )?;
        }
        Ok(())
    }

    /// Maps one `feats.json` record.
    fn feat(&mut self, file: &str, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        let mut description = text(record, file, "desc")?.trim_end().to_owned();
        if let Some(bullets) = optional_array(record, file, "effects_desc")?
            && !bullets.is_empty()
        {
            description.push_str("\n\n");
            let mut rendered = Vec::with_capacity(bullets.len());
            for bullet in bullets {
                rendered.push(
                    bullet
                        .as_str()
                        .ok_or_else(|| {
                            corrupt(
                                file,
                                format!("feat {name} has a non-string effects_desc entry"),
                            )
                        })?
                        .trim_end(),
                );
            }
            description.push_str(&rendered.join("\n"));
        }
        self.push(
            file,
            record,
            Mapped {
                name,
                kind: ContentKind::Feat,
                description,
                data: ContentData::Text,
                notes: Vec::new(),
            },
        )
    }

    /// Maps a text-only record: `conditions.json`, `backgrounds.json` or
    /// `sections.json`.
    fn text(&mut self, file: &str, kind: ContentKind, record: &Value) -> Result<(), ImportError> {
        let name = text(record, file, "name")?;
        self.push(
            file,
            record,
            Mapped {
                name,
                kind,
                description: text(record, file, "desc")?.trim_end().to_owned(),
                data: ContentData::Text,
                notes: Vec::new(),
            },
        )
    }

    /// Adds one mapped entry, deriving the slug and guarding against
    /// collisions.
    fn push(&mut self, file: &str, record: &Value, mapped: Mapped) -> Result<(), ImportError> {
        let Mapped {
            name,
            kind,
            description,
            data,
            notes,
        } = mapped;
        let slug = slugify(&name);
        if slug.is_empty() {
            return Err(corrupt(
                file,
                format!("record {name} produces an empty slug"),
            ));
        }
        let id = RefId::parse(&format!(
            "{}:{}/{slug}",
            self.provenance.namespace.as_str(),
            kind.as_str()
        ))
        .map_err(|error| {
            corrupt(
                file,
                format!("record {name} derives an invalid id: {error}"),
            )
        })?;
        let origin = format!("{file}: {name}");
        if let Some(first) = self.claimed.get(&id) {
            return Err(ImportError::Collision {
                id,
                first: first.clone(),
                second: origin,
            });
        }
        self.claimed.insert(id.clone(), origin);

        let mut ext = Ext::new();
        ext.insert(self.ext_key.clone(), record.clone());
        if !notes.is_empty() {
            self.review.insert(id.clone(), notes.clone());
        }
        self.entries.push(ContentEntry {
            id,
            kind,
            name,
            description,
            data,
            provenance: self.provenance.clone(),
            needs_review: !notes.is_empty(),
            review_notes: notes,
            ext,
        });
        Ok(())
    }

    /// Builds the snapshot and its report.
    fn finish(self) -> SourceSnapshot {
        let mut counts = BTreeMap::new();
        for entry in &self.entries {
            *counts.entry(entry.kind).or_insert(0) += 1;
        }
        let source_revision = self.provenance.source_revision.clone();
        SourceSnapshot {
            provenance: self.provenance,
            entries: self.entries,
            aliases: Vec::new(),
            report: ImportReport {
                format: "open5e".to_owned(),
                source_revision,
                counts,
                review: self.review,
            },
        }
    }
}

/// The result of parsing the `weight` and `cost` fields of an item record.
struct Measures {
    /// Parsed `weight`.
    weight_oz: Option<i32>,
    /// Parsed `cost`.
    value: Option<Coins>,
    /// Notes for values that did not match their pattern.
    notes: Vec<String>,
}

/// Parses `weight` and `cost`, leaving non-matching values `None` with a note.
fn parse_measures(record: &Value) -> Measures {
    let mut notes = Vec::new();
    let weight_oz = match optional_text(record, "weight") {
        Some(raw) => match weight_oz(&raw) {
            Some(ounces) => Some(ounces),
            None => {
                notes.push(format!(
                    "weight {raw:?} did not match <n> lb., <n>/<d> lb. or <n> oz."
                ));
                None
            }
        },
        None => None,
    };
    let value = match optional_text(record, "cost") {
        Some(raw) => match cost(&raw) {
            Some(coins) => Some(coins),
            None => {
                notes.push(format!("cost {raw:?} did not match <n> <cp|sp|ep|gp|pp>"));
                None
            }
        },
        None => None,
    };
    Measures {
        weight_oz,
        value,
        notes,
    }
}

/// Parses `<n> lb.`, `<num>/<den> lb.` (exact ounces) or `<n> oz.`.
fn weight_oz(raw: &str) -> Option<i32> {
    if let Some(pounds) = raw.strip_suffix(" lb.") {
        if let Some((num, den)) = pounds.split_once('/') {
            let num: i32 = num.parse().ok()?;
            let den: i32 = den.parse().ok()?;
            if den == 0 || num.checked_mul(16)? % den != 0 {
                return None;
            }
            return Some(num * 16 / den);
        }
        return pounds.parse::<i32>().ok().map(|pounds| pounds * 16);
    }
    raw.strip_suffix(" oz.")?.parse().ok()
}

/// Parses `<n> <cp|sp|ep|gp|pp>`.
fn cost(raw: &str) -> Option<Coins> {
    let (amount, unit) = raw.split_once(' ')?;
    let amount: i32 = amount.parse().ok()?;
    let mut coins = Coins::default();
    match unit {
        "cp" => coins.cp = amount,
        "sp" => coins.sp = amount,
        "ep" => coins.ep = amount,
        "gp" => coins.gp = amount,
        "pp" => coins.pp = amount,
        _ => return None,
    }
    Some(coins)
}

/// Classifies a magic item by its `type` prefix.
fn magic_item_kind(item_type: &str) -> ItemKind {
    if item_type.starts_with("Weapon") {
        ItemKind::Weapon
    } else if item_type.starts_with("Armor (shield)") {
        ItemKind::Shield
    } else if item_type.starts_with("Armor") {
        ItemKind::Armor
    } else {
        ItemKind::Wondrous
    }
}

/// The slug rule: lowercase ASCII, runs outside `[a-z0-9]` become `-`,
/// leading/trailing `-` trimmed.
fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut pending_dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(character.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    slug
}

/// The spell stat line prefix: `Cantrip` or `3rd-level`.
fn level_label(level: u8) -> String {
    match level {
        0 => "Cantrip".to_owned(),
        1 => "1st-level".to_owned(),
        2 => "2nd-level".to_owned(),
        3 => "3rd-level".to_owned(),
        other => format!("{other}th-level"),
    }
}

/// The `desc` plus `traits`, as two paragraphs.
fn combined_traits(desc: &str, traits: Option<&str>) -> String {
    match traits.filter(|traits| !traits.trim().is_empty()) {
        Some(traits) => format!("{}\n\n{}", desc.trim_end(), traits.trim()),
        None => desc.trim_end().to_owned(),
    }
}

/// Lowercases the first character, for the `Concentration, up to ...` line.
fn lowercase_first(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => {
            let mut lowered: String = first.to_lowercase().collect();
            lowered.push_str(characters.as_str());
            lowered
        }
        None => String::new(),
    }
}

/// Uppercases the first character, for the attunement qualifier.
fn capitalize_first(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => {
            let mut capitalized: String = first.to_uppercase().collect();
            capitalized.push_str(characters.as_str());
            capitalized
        }
        None => String::new(),
    }
}

/// Builds a `Corrupt` error for one dataset file.
fn corrupt(file: &str, message: String) -> ImportError {
    ImportError::Corrupt {
        file: file.to_owned(),
        message,
    }
}

/// The `name` of a record, for diagnostics.
fn record_name(record: &Value) -> &str {
    record
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
}

/// Required field access.
fn field<'a>(value: &'a Value, file: &str, key: &str) -> Result<&'a Value, ImportError> {
    value.get(key).ok_or_else(|| {
        corrupt(
            file,
            format!("record {} is missing {key}", record_name(value)),
        )
    })
}

/// A required string field.
fn text(value: &Value, file: &str, key: &str) -> Result<String, ImportError> {
    field(value, file, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            corrupt(
                file,
                format!("record {} field {key} is not a string", record_name(value)),
            )
        })
}

/// An optional string field; absent and `null` both read as `None`.
fn optional_text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// A required integer field.
fn integer(value: &Value, file: &str, key: &str) -> Result<i64, ImportError> {
    field(value, file, key)?.as_i64().ok_or_else(|| {
        corrupt(
            file,
            format!(
                "record {} field {key} is not an integer",
                record_name(value)
            ),
        )
    })
}

/// A required `i32` field.
fn int32(value: &Value, file: &str, key: &str) -> Result<i32, ImportError> {
    i32::try_from(integer(value, file, key)?).map_err(|_| {
        corrupt(
            file,
            format!("record {} field {key} is out of range", record_name(value)),
        )
    })
}

/// An optional array field; absent and `null` both read as `None`.
fn optional_array<'a>(
    value: &'a Value,
    file: &str,
    key: &str,
) -> Result<Option<&'a [Value]>, ImportError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => found
            .as_array()
            .map(Vec::as_slice)
            .map(Some)
            .ok_or_else(|| {
                corrupt(
                    file,
                    format!("record {} field {key} is not an array", record_name(value)),
                )
            }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use localtt_core::types::{ChallengeRating, Coins, ItemKind, Namespace, RefId};
    use localtt_engine::content::{ContentData, ContentEntry, ContentKind, SpellData};
    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::Open5eImporter;
    use crate::reference::import::{ImportError, Importer};

    fn importer() -> Open5eImporter {
        Open5eImporter::new(
            Namespace::parse("srd2014").expect("a literal namespace"),
            "test-revision".to_owned(),
        )
    }

    fn ref_id(raw: &str) -> RefId {
        RefId::parse(raw).expect("a literal reference")
    }

    fn ext_key() -> Namespace {
        Namespace::parse("src.open5e").expect("a literal namespace")
    }

    fn entry_of<'a>(entries: &'a [ContentEntry], raw: &str) -> &'a ContentEntry {
        let id = ref_id(raw);
        entries
            .iter()
            .find(|entry| entry.id == id)
            .unwrap_or_else(|| panic!("{raw} must convert"))
    }

    fn minimal_records() -> Vec<(&'static str, Value)> {
        vec![
            (
                "spells",
                json!([{
                    "name": "Fireball",
                    "level_int": 3,
                    "school": "Evocation",
                    "concentration": "no",
                    "ritual": "no",
                    "casting_time": "1 action",
                    "range": "150 feet",
                    "components": "V, S, M",
                    "material": "A tiny ball of bat guano and sulfur.",
                    "duration": "Instantaneous",
                    "desc": "A bright streak flashes from your pointing finger.",
                    "higher_level": "The damage increases by 1d6 per slot level above 3rd."
                }]),
            ),
            (
                "monsters",
                json!([{
                    "name": "Goblin",
                    "armor_class": 15,
                    "hit_points": 7,
                    "challenge_rating": "1/4",
                    "strength": 8,
                    "dexterity": 14,
                    "constitution": 10,
                    "intelligence": 10,
                    "wisdom": 8,
                    "charisma": 8,
                    "special_abilities": [{
                        "name": "Nimble Escape",
                        "desc": "The goblin can take the Disengage or Hide action."
                    }],
                    "actions": [{
                        "name": "Scimitar",
                        "desc": "Melee Weapon Attack: +4 to hit, 1d6+2 slashing damage."
                    }]
                }]),
            ),
            (
                "magicitems",
                json!([{
                    "name": "Wand of Fireballs",
                    "desc": "This wand has 7 charges.",
                    "type": "Wand",
                    "requires-attunement": "requires attunement by a spellcaster"
                }]),
            ),
            (
                "weapons",
                json!([{
                    "name": "Longsword",
                    "category": "Martial Melee Weapons",
                    "cost": "15 gp",
                    "weight": "3 lb.",
                    "damage_dice": "1d8",
                    "damage_type": "slashing",
                    "properties": ["Versatile"]
                }]),
            ),
            (
                "armor",
                json!([{
                    "name": "Shield",
                    "category": "Shield",
                    "cost": "10 gp",
                    "weight": "6 lb."
                }]),
            ),
            (
                "conditions",
                json!([{"name": "Prone", "desc": "You can't stand, only crawl."}]),
            ),
            (
                "classes",
                json!([{
                    "name": "Fighter",
                    "features": {"desc": "A master of martial combat."},
                    "subtypes": [{"name": "Champion", "desc": "The archetypal Champion."}]
                }]),
            ),
            (
                "races",
                json!([{
                    "name": "Elf",
                    "desc": "Elves are a magical people.",
                    "traits": "Darkvision: 60 feet.",
                    "subtypes": [{"name": "High Elf", "desc": "Keen mind."}]
                }]),
            ),
            (
                "backgrounds",
                json!([{"name": "Acolyte", "desc": "You have spent your life in a temple."}]),
            ),
            (
                "feats",
                json!([{
                    "name": "Sharpshooter",
                    "desc": "You are a master of ranged weapons.",
                    "effects_desc": ["- Attacking at long range doesn't impose disadvantage."]
                }]),
            ),
            (
                "sections",
                json!([{"name": "Cover", "desc": "Walls and trees can provide cover."}]),
            ),
        ]
    }

    fn write_dataset(records: &[(&str, Value)]) -> TempDir {
        let dir = TempDir::new().expect("a temp dir");
        for (name, value) in records {
            let path = dir.path().join(format!("{name}.json"));
            fs::write(&path, serde_json::to_vec(value).expect("a record")).expect("a dataset file");
        }
        dir
    }

    #[test]
    fn imports_a_minimal_dataset() {
        let dir = write_dataset(&minimal_records());
        let snapshot = importer().import(dir.path()).expect("the dataset converts");

        assert_eq!(importer().format(), "open5e");
        assert_eq!(snapshot.provenance.namespace.as_str(), "srd2014");
        assert_eq!(snapshot.provenance.source_revision, "test-revision");
        assert_eq!(snapshot.provenance.license, "CC-BY-4.0");
        assert!(
            snapshot
                .provenance
                .attribution
                .contains("System Reference Document 5.1")
        );
        assert_eq!(snapshot.report.format, "open5e");
        assert_eq!(snapshot.entries.len(), 13);
        assert_eq!(snapshot.report.counts[&ContentKind::Item], 3);
        assert_eq!(snapshot.report.counts[&ContentKind::Race], 2);
        assert_eq!(snapshot.report.counts[&ContentKind::Spell], 1);
        assert!(snapshot.aliases.is_empty());
        assert!(snapshot.report.review.is_empty());
        assert!(snapshot.entries.iter().all(|entry| !entry.needs_review));

        let fireball = entry_of(&snapshot.entries, "srd2014:spell/fireball");
        assert!(fireball.description.contains("bright streak"));
        assert!(fireball.description.contains("**At Higher Levels.**"));
        assert_eq!(
            fireball.data,
            ContentData::Spell(SpellData {
                level: 3,
                school: "Evocation".to_owned(),
                concentration: false,
                ritual: false,
            })
        );
        assert_eq!(fireball.ext[&ext_key()]["name"], "Fireball");

        let goblin = entry_of(&snapshot.entries, "srd2014:monster/goblin");
        assert!(goblin.description.starts_with("**Nimble Escape.**"));
        assert!(goblin.description.contains("**Scimitar.**"));
        match &goblin.data {
            ContentData::Monster(monster) => {
                assert_eq!(monster.abilities.dexterity, 14);
                assert_eq!(monster.armor_class, 15);
                assert_eq!(monster.hit_points, 7);
                assert_eq!(
                    monster.challenge_rating,
                    ChallengeRating::from_eighths(2).expect("1/4")
                );
            }
            other => panic!("expected a monster payload, got {other:?}"),
        }

        let wand = entry_of(&snapshot.entries, "srd2014:item/wand-of-fireballs");
        assert!(
            wand.description
                .contains("*Requires attunement by a spellcaster*"),
            "{}",
            wand.description
        );
        match &wand.data {
            ContentData::Item(item) => {
                assert_eq!(item.kind, ItemKind::Wondrous);
                assert!(item.requires_attunement);
                assert!(item.weight_oz.is_none());
                assert!(item.value.is_none());
            }
            other => panic!("expected an item payload, got {other:?}"),
        }

        let sword = entry_of(&snapshot.entries, "srd2014:item/longsword");
        match &sword.data {
            ContentData::Item(item) => {
                assert_eq!(item.kind, ItemKind::Weapon);
                assert_eq!(item.weight_oz, Some(48));
                assert_eq!(
                    item.value,
                    Some(Coins {
                        gp: 15,
                        ..Coins::default()
                    })
                );
            }
            other => panic!("expected an item payload, got {other:?}"),
        }

        let shield = entry_of(&snapshot.entries, "srd2014:item/shield");
        match &shield.data {
            ContentData::Item(item) => {
                assert_eq!(item.kind, ItemKind::Shield);
                assert_eq!(item.weight_oz, Some(96));
            }
            other => panic!("expected an item payload, got {other:?}"),
        }

        assert_eq!(
            entry_of(&snapshot.entries, "srd2014:class/fighter").description,
            "A master of martial combat."
        );
        assert_eq!(
            entry_of(&snapshot.entries, "srd2014:subclass/champion").description,
            "The archetypal Champion."
        );
        assert_eq!(
            entry_of(&snapshot.entries, "srd2014:race/elf").description,
            "Elves are a magical people.\n\nDarkvision: 60 feet."
        );
        assert_eq!(
            entry_of(&snapshot.entries, "srd2014:race/high-elf").description,
            "Keen mind."
        );
        assert_eq!(
            entry_of(&snapshot.entries, "srd2014:feat/sharpshooter").description,
            "You are a master of ranged weapons.\n\n- Attacking at long range doesn't impose disadvantage."
        );
    }

    #[test]
    fn concentration_and_ritual_flags_map() {
        let mut records = minimal_records();
        for (file, value) in &mut records {
            if *file == "spells" {
                value[0]["concentration"] = json!("yes");
                value[0]["duration"] = json!("Up to 1 minute");
                value[0]["ritual"] = json!("yes");
            }
        }
        let dir = write_dataset(&records);
        let snapshot = importer().import(dir.path()).expect("the dataset converts");
        let fireball = entry_of(&snapshot.entries, "srd2014:spell/fireball");

        assert!(
            fireball
                .description
                .contains("**Duration:** Concentration, up to 1 minute"),
            "{}",
            fireball.description
        );
        assert_eq!(
            fireball.data,
            ContentData::Spell(SpellData {
                level: 3,
                school: "Evocation".to_owned(),
                concentration: true,
                ritual: true,
            })
        );
    }

    #[test]
    fn a_missing_file_is_unavailable() {
        let records = minimal_records();
        let dir = write_dataset(&records[1..]);

        match importer().import(dir.path()) {
            Err(ImportError::Unavailable { message }) => {
                assert!(message.contains("spells.json"), "{message}");
            }
            other => panic!("a missing dataset file must be unavailable, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_is_corrupt() {
        let dir = write_dataset(&minimal_records());
        fs::write(dir.path().join("spells.json"), b"{ not json").expect("a broken file");

        match importer().import(dir.path()) {
            Err(ImportError::Corrupt { file, .. }) => assert_eq!(file, "spells.json"),
            other => panic!("malformed json must be corrupt, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_slug_is_corrupt() {
        let mut records = minimal_records();
        for (file, value) in &mut records {
            if *file == "spells" {
                value[0]["name"] = json!("!!!");
            }
        }
        let dir = write_dataset(&records);

        match importer().import(dir.path()) {
            Err(ImportError::Corrupt { file, message }) => {
                assert_eq!(file, "spells.json");
                assert!(message.contains("empty slug"), "{message}");
            }
            other => panic!("an empty slug must be corrupt, got {other:?}"),
        }
    }

    #[test]
    fn a_collision_names_both_records() {
        let mut records = minimal_records();
        for (file, value) in &mut records {
            if *file == "spells" {
                let mut second = value[0].clone();
                second["name"] = json!("Fire-Ball");
                value[0]["name"] = json!("Fire Ball");
                value
                    .as_array_mut()
                    .expect("an array of spells")
                    .push(second);
            }
        }
        let dir = write_dataset(&records);

        match importer().import(dir.path()) {
            Err(ImportError::Collision { id, first, second }) => {
                assert_eq!(id, ref_id("srd2014:spell/fire-ball"));
                assert_eq!(first, "spells.json: Fire Ball");
                assert_eq!(second, "spells.json: Fire-Ball");
            }
            other => panic!("a slug collision must be a collision, got {other:?}"),
        }
    }

    #[test]
    fn unparsable_measures_get_review_notes() {
        let mut records = minimal_records();
        for (file, value) in &mut records {
            if *file == "weapons" {
                value[0]["weight"] = json!("3 stone");
                value[0]["cost"] = json!("a handful of gold");
            }
        }
        let dir = write_dataset(&records);
        let snapshot = importer().import(dir.path()).expect("the dataset converts");
        let sword = entry_of(&snapshot.entries, "srd2014:item/longsword");

        assert!(sword.needs_review);
        assert_eq!(sword.review_notes.len(), 2);
        assert_eq!(snapshot.report.review[&sword.id], sword.review_notes);
        match &sword.data {
            ContentData::Item(item) => {
                assert!(item.weight_oz.is_none());
                assert!(item.value.is_none());
            }
            other => panic!("expected an item payload, got {other:?}"),
        }
    }

    #[test]
    fn a_bad_challenge_rating_is_corrupt() {
        let mut records = minimal_records();
        for (file, value) in &mut records {
            if *file == "monsters" {
                value[0]["challenge_rating"] = json!("CR 1/4");
            }
        }
        let dir = write_dataset(&records);

        match importer().import(dir.path()) {
            Err(ImportError::Corrupt { file, .. }) => assert_eq!(file, "monsters.json"),
            other => panic!("a bad challenge rating must be corrupt, got {other:?}"),
        }
    }
}
