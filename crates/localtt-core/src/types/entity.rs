//! The entity header, its type-stated and runtime-tagged forms, and the body family.

use std::{fmt, hash::Hash};

use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::DeserializeOwned,
    ser::SerializeStruct,
};

use crate::types::{
    AclEntry,
    EntityError,
    EntityId,
    Ext,
    InlineItem,
    PlayerId,
    RulesetId,
    SchemaVersion,
    StatblockBody,
    UnixTime,
};

/// The eight kinds of entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// Player character.
    Pc,
    /// Companion creature.
    Companion,
    /// Summoned creature.
    Summon,
    /// Familiar.
    Familiar,
    /// Non-player character.
    Npc,
    /// Monster from a content pack.
    Monster,
    /// Session note.
    Note,
    /// Handout.
    Handout,
}

/// The three body families; six entity kinds share the statblock family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// Character/creature sheet.
    Statblock,
    /// Session note.
    Note,
    /// Handout.
    Handout,
}

impl EntityKind {
    /// The body family this kind carries.
    #[must_use]
    #[inline]
    pub const fn body_kind(self) -> BodyKind {
        match self {
            Self::Pc
            | Self::Companion
            | Self::Summon
            | Self::Familiar
            | Self::Npc
            | Self::Monster => BodyKind::Statblock,
            Self::Note => BodyKind::Note,
            Self::Handout => BodyKind::Handout,
        }
    }
}

pub(crate) mod sealed {
    /// Prevents downstream implementations of [`super::Body`].
    #[expect(
        unnameable_types,
        reason = "deliberate sealed-trait pattern (core DESIGN.md section 3.4): reachable to satisfy the public Body bound, nameable by nobody"
    )]
    pub trait Sealed {}
}

/// Sealed marker trait implemented only by the three body types.
pub trait Body:
    sealed::Sealed + Clone + fmt::Debug + PartialEq + Eq + Send + Sync + Serialize + DeserializeOwned
{
    /// The body family this type belongs to.
    const BODY_KIND: BodyKind;
}

/// Common header plus a statically known body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity<B: Body> {
    /// Entity kind; validated against the body family at construction and parse,
    /// and only changeable through [`Entity::set_kind`].
    kind: EntityKind,
    /// Entity id.
    pub id: EntityId,
    /// Ruleset the sheet follows.
    pub ruleset: RulesetId,
    /// Display name.
    pub name: String,
    /// Owning player.
    pub owner: PlayerId,
    /// Parent entity, e.g. a summon's caster.
    pub parent: Option<EntityId>,
    /// Access grants.
    pub acl: Vec<AclEntry>,
    /// Revision counter for optimistic concurrency.
    pub rev: u64,
    /// Document schema version.
    pub schema_version: SchemaVersion,
    /// Plugin payload slot.
    pub ext: Ext,
    /// When the entity was created (wall clock).
    pub created_at: UnixTime,
    /// When the entity was last updated (wall clock).
    pub updated_at: UnixTime,
    /// The body payload.
    pub body: B,
}

/// A character or creature entity.
pub type StatblockEntity = Entity<StatblockBody>;
/// A session note entity.
pub type NoteEntity = Entity<NoteBody>;
/// A handout entity.
pub type HandoutEntity = Entity<HandoutBody>;

impl<B: Body> Entity<B> {
    /// Copies the header out of the entity, leaving the body behind.
    #[must_use]
    #[inline]
    pub fn header(&self) -> EntityHeader {
        EntityHeader {
            id: self.id,
            kind: self.kind,
            name: self.name.clone(),
            owner: self.owner,
            parent: self.parent,
            acl: self.acl.clone(),
            rev: self.rev,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    /// Creates a builder with defaults (rev 0, current schema version, empty
    /// ACL/ext); the kind/body pairing is checked in `build`.
    #[inline]
    pub fn builder(
        kind: EntityKind,
        id: EntityId,
        name: impl Into<String>,
        owner: PlayerId,
        body: B,
    ) -> EntityBuilder<B> {
        EntityBuilder {
            kind,
            id,
            name: name.into(),
            owner,
            body,
            ruleset: None,
            parent: None,
            acl: Vec::new(),
            rev: 0,
            schema_version: SchemaVersion::CURRENT,
            ext: Ext::new(),
            created_at: None,
            updated_at: None,
        }
    }

    /// The entity kind.
    #[must_use]
    #[inline]
    pub fn kind(&self) -> EntityKind {
        self.kind
    }

    /// Changes the entity kind, rejecting a kind from another body family.
    ///
    /// The kind/body pairing is fixed when the entity is built or parsed, so
    /// this is the only way to change it afterwards, for example to promote a
    /// template into [`EntityKind::Summon`]. A same-family kind is always
    /// accepted.
    #[inline]
    pub fn set_kind(&mut self, kind: EntityKind) -> Result<(), EntityError> {
        if kind.body_kind() != B::BODY_KIND {
            return Err(EntityError::KindBodyMismatch {
                kind,
                body_kind: B::BODY_KIND,
            });
        }
        self.kind = kind;
        Ok(())
    }
}

/// Builder for [`Entity`] documents.
pub struct EntityBuilder<B: Body> {
    kind: EntityKind,
    id: EntityId,
    name: String,
    owner: PlayerId,
    body: B,
    ruleset: Option<RulesetId>,
    parent: Option<EntityId>,
    acl: Vec<AclEntry>,
    rev: u64,
    schema_version: SchemaVersion,
    ext: Ext,
    created_at: Option<UnixTime>,
    updated_at: Option<UnixTime>,
}

impl<B: Body> EntityBuilder<B> {
    /// Sets the ruleset.
    #[inline]
    pub fn ruleset(mut self, r: RulesetId) -> Self {
        self.ruleset = Some(r);
        self
    }

    /// Sets the parent entity.
    #[inline]
    pub fn parent(mut self, p: EntityId) -> Self {
        self.parent = Some(p);
        self
    }

    /// Sets the ACL.
    #[inline]
    pub fn acl(mut self, acl: Vec<AclEntry>) -> Self {
        self.acl = acl;
        self
    }

    /// Sets the revision counter.
    #[inline]
    pub fn rev(mut self, rev: u64) -> Self {
        self.rev = rev;
        self
    }

    /// Sets the schema version.
    ///
    /// The value is kept in memory and written to the wire, but a document is
    /// re-stamped at [`SchemaVersion::CURRENT`] when it is loaded (section 5),
    /// so the setter simulates an older or newer file rather than pinning a
    /// stored version.
    #[inline]
    pub fn schema_version(mut self, v: SchemaVersion) -> Self {
        self.schema_version = v;
        self
    }

    /// Sets the plugin payload slot.
    #[inline]
    pub fn ext(mut self, ext: Ext) -> Self {
        self.ext = ext;
        self
    }

    /// Sets the creation time.
    #[inline]
    pub fn created_at(mut self, t: UnixTime) -> Self {
        self.created_at = Some(t);
        self
    }

    /// Sets the last-update time.
    #[inline]
    pub fn updated_at(mut self, t: UnixTime) -> Self {
        self.updated_at = Some(t);
        self
    }

    /// Fails with [`EntityError::KindBodyMismatch`] unless
    /// `kind.body_kind() == B::BODY_KIND`.
    #[inline]
    pub fn build(self) -> Result<Entity<B>, EntityError> {
        if self.kind.body_kind() != B::BODY_KIND {
            return Err(EntityError::KindBodyMismatch {
                kind: self.kind,
                body_kind: B::BODY_KIND,
            });
        }
        Ok(Entity {
            kind: self.kind,
            id: self.id,
            // Core is I/O-free: an unset ruleset or timestamp falls back to the
            // default ruleset or the epoch instead of contacting a clock.
            ruleset: self.ruleset.unwrap_or_else(RulesetId::default_5e),
            name: self.name,
            owner: self.owner,
            parent: self.parent,
            acl: self.acl,
            rev: self.rev,
            schema_version: self.schema_version,
            ext: self.ext,
            created_at: self.created_at.unwrap_or(UnixTime::from_seconds(0)),
            updated_at: self.updated_at.unwrap_or(UnixTime::from_seconds(0)),
            body: self.body,
        })
    }
}

/// Wire shape of [`Entity`]: a flat header plus a nested `"body"` object.
///
/// Deserialization only; serialization streams fields directly so the body is
/// never cloned.
///
/// `schema_version` is deliberately absent: the version gate in
/// `SchemaVersion::accepts` has already run, and `DESIGN.md` section 5 says a
/// document loaded by this build is re-stamped at this build's version (a save
/// from the older build drops the newer minor's unknown fields).
#[derive(Deserialize)]
#[serde(bound(deserialize = "B: Deserialize<'de>"))]
struct EntityWire<B> {
    /// Absent when supplied by an outer internally-tagged dispatch.
    #[serde(default)]
    kind: Option<EntityKind>,
    id: EntityId,
    ruleset: RulesetId,
    name: String,
    owner: PlayerId,
    #[serde(default)]
    parent: Option<EntityId>,
    #[serde(default)]
    acl: Vec<AclEntry>,
    #[serde(default)]
    rev: u64,
    #[serde(default)]
    ext: Ext,
    created_at: UnixTime,
    updated_at: UnixTime,
    body: B,
}

impl<B: Body> Serialize for Entity<B> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The `parent` field is omitted when absent, so the hint must count 12
        // fields plus `parent` only when it is written.
        let fields = 12 + usize::from(self.parent.is_some());
        let mut state = serializer.serialize_struct("Entity", fields)?;
        state.serialize_field("kind", &self.kind)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("ruleset", &self.ruleset)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("owner", &self.owner)?;
        if self.parent.is_some() {
            state.serialize_field("parent", &self.parent)?;
        }
        state.serialize_field("acl", &self.acl)?;
        state.serialize_field("rev", &self.rev)?;
        state.serialize_field("schema_version", &self.schema_version)?;
        state.serialize_field("ext", &self.ext)?;
        state.serialize_field("created_at", &self.created_at)?;
        state.serialize_field("updated_at", &self.updated_at)?;
        state.serialize_field("body", &self.body)?;
        state.end()
    }
}

impl<B: Body> EntityWire<B> {
    /// Validates the kind/body pairing and builds the entity.
    fn into_entity(self, kind: EntityKind) -> Result<Entity<B>, EntityError> {
        if kind.body_kind() != B::BODY_KIND {
            return Err(EntityError::KindBodyMismatch {
                kind,
                body_kind: B::BODY_KIND,
            });
        }
        Ok(Entity {
            kind,
            id: self.id,
            ruleset: self.ruleset,
            name: self.name,
            owner: self.owner,
            parent: self.parent,
            acl: self.acl,
            rev: self.rev,
            schema_version: SchemaVersion::CURRENT,
            ext: self.ext,
            created_at: self.created_at,
            updated_at: self.updated_at,
            body: self.body,
        })
    }
}

impl<'de, B: Body> Deserialize<'de> for Entity<B> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = EntityWire::<B>::deserialize(deserializer)?;
        let kind = wire
            .kind
            .ok_or_else(|| serde::de::Error::missing_field("kind"))?;
        wire.into_entity(kind).map_err(serde::de::Error::custom)
    }
}

/// Runtime-tagged entity for storage, iteration and `view_of`, where the body
/// kind is only known at runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (core DESIGN.md section 3.4); boxing the statblock variant changes the public shape, deferred to a later pass"
)]
pub enum AnyEntity {
    /// A statblock entity.
    Statblock(Entity<StatblockBody>),
    /// A note entity.
    Note(Entity<NoteBody>),
    /// A handout entity.
    Handout(Entity<HandoutBody>),
}

/// The common header of an entity without its body: everything a listing,
/// an access check or an index needs, copied out of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityHeader {
    /// The entity id.
    pub id: EntityId,
    /// The entity kind.
    pub kind: EntityKind,
    /// Display name.
    pub name: String,
    /// The owning player.
    pub owner: PlayerId,
    /// The owning entity, for summons and mounts.
    pub parent: Option<EntityId>,
    /// Read/write grants beyond the owner.
    pub acl: Vec<AclEntry>,
    /// Optimistic-concurrency revision.
    pub rev: u64,
    /// When the entity was created.
    pub created_at: UnixTime,
    /// Wall-clock stamp of the last write.
    pub updated_at: UnixTime,
}

impl AnyEntity {
    /// The entity id.
    #[must_use]
    #[inline]
    pub fn id(&self) -> EntityId {
        match self {
            Self::Statblock(e) => e.id,
            Self::Note(e) => e.id,
            Self::Handout(e) => e.id,
        }
    }

    /// The entity kind.
    #[must_use]
    #[inline]
    pub fn kind(&self) -> EntityKind {
        match self {
            Self::Statblock(e) => e.kind(),
            Self::Note(e) => e.kind(),
            Self::Handout(e) => e.kind(),
        }
    }

    /// The display name.
    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        match self {
            Self::Statblock(e) => &e.name,
            Self::Note(e) => &e.name,
            Self::Handout(e) => &e.name,
        }
    }

    /// The owning player.
    #[must_use]
    #[inline]
    pub fn owner(&self) -> PlayerId {
        match self {
            Self::Statblock(e) => e.owner,
            Self::Note(e) => e.owner,
            Self::Handout(e) => e.owner,
        }
    }

    /// The parent entity, if any.
    #[must_use]
    #[inline]
    pub fn parent(&self) -> Option<EntityId> {
        match self {
            Self::Statblock(e) => e.parent,
            Self::Note(e) => e.parent,
            Self::Handout(e) => e.parent,
        }
    }

    /// The access grants.
    #[must_use]
    #[inline]
    pub fn acl(&self) -> &[AclEntry] {
        match self {
            Self::Statblock(e) => &e.acl,
            Self::Note(e) => &e.acl,
            Self::Handout(e) => &e.acl,
        }
    }

    /// The revision counter.
    #[must_use]
    #[inline]
    pub fn rev(&self) -> u64 {
        match self {
            Self::Statblock(e) => e.rev,
            Self::Note(e) => e.rev,
            Self::Handout(e) => e.rev,
        }
    }

    /// The document schema version.
    #[must_use]
    #[inline]
    pub fn schema_version(&self) -> SchemaVersion {
        match self {
            Self::Statblock(e) => e.schema_version,
            Self::Note(e) => e.schema_version,
            Self::Handout(e) => e.schema_version,
        }
    }

    /// The ruleset.
    #[must_use]
    #[inline]
    pub fn ruleset(&self) -> &RulesetId {
        match self {
            Self::Statblock(e) => &e.ruleset,
            Self::Note(e) => &e.ruleset,
            Self::Handout(e) => &e.ruleset,
        }
    }

    /// The plugin payload slot.
    #[must_use]
    #[inline]
    pub fn ext(&self) -> &Ext {
        match self {
            Self::Statblock(e) => &e.ext,
            Self::Note(e) => &e.ext,
            Self::Handout(e) => &e.ext,
        }
    }

    /// When the entity was created.
    #[must_use]
    #[inline]
    pub fn created_at(&self) -> UnixTime {
        match self {
            Self::Statblock(e) => e.created_at,
            Self::Note(e) => e.created_at,
            Self::Handout(e) => e.created_at,
        }
    }

    /// When the entity was last updated.
    #[must_use]
    #[inline]
    pub fn updated_at(&self) -> UnixTime {
        match self {
            Self::Statblock(e) => e.updated_at,
            Self::Note(e) => e.updated_at,
            Self::Handout(e) => e.updated_at,
        }
    }

    /// The body-less header of the entity.
    #[must_use]
    #[inline]
    pub fn header(&self) -> EntityHeader {
        match self {
            Self::Statblock(e) => e.header(),
            Self::Note(e) => e.header(),
            Self::Handout(e) => e.header(),
        }
    }

    /// The statblock entity, when the body is a statblock.
    #[must_use]
    #[inline]
    pub fn as_statblock(&self) -> Option<&Entity<StatblockBody>> {
        match self {
            Self::Statblock(e) => Some(e),
            Self::Note(_) | Self::Handout(_) => None,
        }
    }

    /// The note entity, when the body is a note.
    #[must_use]
    #[inline]
    pub fn as_note(&self) -> Option<&Entity<NoteBody>> {
        match self {
            Self::Note(e) => Some(e),
            Self::Statblock(_) | Self::Handout(_) => None,
        }
    }

    /// The handout entity, when the body is a handout.
    #[must_use]
    #[inline]
    pub fn as_handout(&self) -> Option<&Entity<HandoutBody>> {
        match self {
            Self::Handout(e) => Some(e),
            Self::Statblock(_) | Self::Note(_) => None,
        }
    }
}

impl From<Entity<StatblockBody>> for AnyEntity {
    #[inline]
    fn from(entity: Entity<StatblockBody>) -> Self {
        Self::Statblock(entity)
    }
}

impl From<Entity<NoteBody>> for AnyEntity {
    #[inline]
    fn from(entity: Entity<NoteBody>) -> Self {
        Self::Note(entity)
    }
}

impl From<Entity<HandoutBody>> for AnyEntity {
    #[inline]
    fn from(entity: Entity<HandoutBody>) -> Self {
        Self::Handout(entity)
    }
}

/// Runtime dispatch by the document's top-level `"kind"` field.
///
/// The tag is consumed by the enum, so each payload is the kind-less wire shape.
///
/// One variant per [`EntityKind`]: serde's internally-tagged representation
/// cannot derive the kind-to-body mapping, so this list must stay in sync with
/// `EntityKind` (a new kind otherwise serializes but fails to deserialize). The
/// `any_entity_round_trips_every_kind` test covers the current set.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AnyEntityWire {
    Pc(EntityWire<StatblockBody>),
    Companion(EntityWire<StatblockBody>),
    Summon(EntityWire<StatblockBody>),
    Familiar(EntityWire<StatblockBody>),
    Npc(EntityWire<StatblockBody>),
    Monster(EntityWire<StatblockBody>),
    Note(EntityWire<NoteBody>),
    Handout(EntityWire<HandoutBody>),
}

/// Builds an entity from a kind-less wire shape, mapping the pairing error.
fn assemble<B: Body, E: serde::de::Error>(
    wire: EntityWire<B>,
    kind: EntityKind,
) -> Result<Entity<B>, E> {
    wire.into_entity(kind).map_err(serde::de::Error::custom)
}

impl Serialize for AnyEntity {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Statblock(e) => e.serialize(serializer),
            Self::Note(e) => e.serialize(serializer),
            Self::Handout(e) => e.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AnyEntity {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = AnyEntityWire::deserialize(deserializer)?;
        Ok(match wire {
            AnyEntityWire::Pc(w) => Self::Statblock(assemble(w, EntityKind::Pc)?),
            AnyEntityWire::Companion(w) => Self::Statblock(assemble(w, EntityKind::Companion)?),
            AnyEntityWire::Summon(w) => Self::Statblock(assemble(w, EntityKind::Summon)?),
            AnyEntityWire::Familiar(w) => Self::Statblock(assemble(w, EntityKind::Familiar)?),
            AnyEntityWire::Npc(w) => Self::Statblock(assemble(w, EntityKind::Npc)?),
            AnyEntityWire::Monster(w) => Self::Statblock(assemble(w, EntityKind::Monster)?),
            AnyEntityWire::Note(w) => Self::Note(assemble(w, EntityKind::Note)?),
            AnyEntityWire::Handout(w) => Self::Handout(assemble(w, EntityKind::Handout)?),
        })
    }
}

/// Markdown note, opaque to core, size-capped by the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteBody {
    /// The note text.
    pub text: String,
}

impl sealed::Sealed for NoteBody {}

impl Body for NoteBody {
    const BODY_KIND: BodyKind = BodyKind::Note;
}

/// Handout payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoutBody {
    /// The handout content.
    pub content: HandoutContent,
}

impl sealed::Sealed for HandoutBody {}

impl Body for HandoutBody {
    const BODY_KIND: BodyKind = BodyKind::Handout;
}

/// A handout is either free text or an item definition ("give").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "document-shaped payload enum (core DESIGN.md section 3.6); boxing the item variant is an API decision deferred to a later pass"
)]
pub enum HandoutContent {
    /// Free text shown to the recipient.
    Text {
        /// The text.
        text: String,
    },
    /// The item lands in the recipient's inventory as their own copy.
    Item {
        /// The item definition.
        item: InlineItem,
    },
}
