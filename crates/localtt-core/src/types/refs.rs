//! Validated ruleset, namespace and content-reference strings, and the `ext` payload slot.

use std::{collections::BTreeMap, fmt, hash::Hash};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::{NamespaceError, RefIdError, RulesetIdError};

/// Ruleset identifier, e.g. `"5e-2014"`: lowercase `[a-z0-9-]`, at most 32
/// characters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RulesetId(String);

impl RulesetId {
    /// The default 5e (2014) ruleset id string.
    pub const DND5E_2014: &'static str = "5e-2014";

    /// Parses and validates a ruleset id.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, RulesetIdError> {
        if raw.len() > 32 || !is_lower_kebab(raw) {
            return Err(RulesetIdError::Invalid);
        }
        Ok(Self(raw.to_owned()))
    }

    /// The ruleset id string.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The default 5e (2014) ruleset, for a builder that never set one.
    ///
    /// Built directly from the validated [`Self::DND5E_2014`] constant rather
    /// than re-parsing it, so no `expect` is needed on the hot path. The
    /// constant's validity is pinned by a test.
    #[inline]
    pub(crate) fn default_5e() -> Self {
        Self(Self::DND5E_2014.to_owned())
    }
}

impl fmt::Display for RulesetId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for RulesetId {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RulesetId {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Plugin data namespace, e.g. `"com.example.foo"`: lowercase dotted segments.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(String);

impl Namespace {
    /// Parses and validates a namespace.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, NamespaceError> {
        if !is_namespace(raw) {
            return Err(NamespaceError::Invalid);
        }
        Ok(Self(raw.to_owned()))
    }

    /// The namespace string.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Namespace {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Namespace {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Namespace {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Namespaced content reference: `<source>:<kind>/<slug>`, e.g.
/// `"srd2014:spell/fireball"` or `"pack.mytome:item/foo"`.
///
/// Internals are the canonical string plus two segment boundaries: one
/// allocation per reference.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RefId {
    raw: String,
    namespace_end: u32,
    kind_end: u32,
}

impl RefId {
    /// Parses and validates a reference.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, RefIdError> {
        let (namespace, rest) = raw.split_once(':').ok_or(RefIdError::Invalid)?;
        let (kind, slug) = rest.split_once('/').ok_or(RefIdError::Invalid)?;
        if !is_namespace(namespace) || !is_segment(kind) || !is_segment(slug) {
            return Err(RefIdError::Invalid);
        }
        Ok(Self {
            raw: raw.to_owned(),
            namespace_end: namespace.len() as u32,
            kind_end: (namespace.len() + 1 + kind.len()) as u32,
        })
    }

    /// The source namespace, e.g. `"srd2014"` or `"pack.mytome"`.
    #[must_use]
    #[inline]
    pub fn namespace(&self) -> &str {
        &self.raw[..self.namespace_end as usize]
    }

    /// The content kind, e.g. `"spell"`.
    #[must_use]
    #[inline]
    pub fn kind(&self) -> &str {
        &self.raw[self.namespace_end as usize + 1..self.kind_end as usize]
    }

    /// The content slug, e.g. `"fireball"`.
    #[must_use]
    #[inline]
    pub fn slug(&self) -> &str {
        &self.raw[self.kind_end as usize + 1..]
    }

    /// The canonical `<source>:<kind>/<slug>` string.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl fmt::Display for RefId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl Serialize for RefId {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for RefId {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// True when every dotted segment of `s` is a non-empty lower-kebab segment.
fn is_namespace(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(is_segment)
}

/// True when `s` is a non-empty run of `[a-z0-9_-]`.
pub(super) fn is_segment(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// True when `s` is a non-empty run of `[a-z0-9-]`.
fn is_lower_kebab(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A reference to shipped/pack content plus its name snapshot, so a missing
/// pack degrades to text instead of a broken sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRef {
    /// The content reference.
    pub id: RefId,
    /// The display name captured at reference time.
    pub name: String,
}

/// Either a reference or inline text (identity boxes, subclasses, spell
/// sources).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RefOrText {
    /// A reference to shipped/pack content.
    Ref(ContentRef),
    /// Inline free text.
    Text(String),
}

/// Plugin payload slot: namespace to arbitrary JSON.
pub type Ext = BTreeMap<Namespace, Json>;
pub use serde_json::Value as Json;
