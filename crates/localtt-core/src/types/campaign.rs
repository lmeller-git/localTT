//! The campaign document: clock, settings, initiative and the player roster.

use std::hash::Hash;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::{
    Audience,
    CampaignId,
    CampaignTime,
    EntityId,
    Ext,
    PlayerId,
    RulesetId,
    SchemaVersion,
    TokenHashError,
    UnixTime,
};

/// Singleton campaign document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Campaign {
    /// Campaign id.
    pub id: CampaignId,
    /// Display name.
    pub name: String,
    /// Document schema version.
    pub schema_version: SchemaVersion,
    /// Default ruleset for new entities.
    pub default_ruleset: RulesetId,
    /// The campaign clock.
    pub clock: Clock,
    /// Campaign-wide settings.
    pub settings: CampaignSettings,
    /// The initiative order for the current encounter.
    #[serde(default)]
    pub initiative: Initiative,
    /// The participants of the campaign.
    pub players: Vec<Player>,
    /// Plugin payload slot.
    pub ext: Ext,
}

/// Wire shape of [`Campaign`] without the version stamp; see the
/// [`Deserialize`] impl for why `schema_version` is re-stamped on load.
#[derive(Deserialize)]
struct CampaignWire {
    id: CampaignId,
    name: String,
    default_ruleset: RulesetId,
    clock: Clock,
    settings: CampaignSettings,
    #[serde(default)]
    initiative: Initiative,
    players: Vec<Player>,
    #[serde(default)]
    ext: Ext,
}

impl<'de> Deserialize<'de> for Campaign {
    /// Deserializes a campaign, re-stamping `schema_version` at
    /// [`SchemaVersion::CURRENT`].
    ///
    /// The version gate (`SchemaVersion::accepts`) has already run, and
    /// `DESIGN.md` section 5 says a document loaded by this build is stored at
    /// this build's version: a save from the older build drops the newer
    /// minor's unknown fields, so keeping the file's value would be a lie.
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = CampaignWire::deserialize(deserializer)?;
        Ok(Self {
            id: wire.id,
            name: wire.name,
            schema_version: SchemaVersion::CURRENT,
            default_ruleset: wire.default_ruleset,
            clock: wire.clock,
            settings: wire.settings,
            initiative: wire.initiative,
            players: wire.players,
            ext: wire.ext,
        })
    }
}

impl Campaign {
    /// Looks up a participant by id; `None` when the id is not on the roster.
    #[must_use]
    #[inline]
    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|seat| seat.id == id)
    }
}

/// The DM-controlled campaign clock; rounds and the in-game calendar (day,
/// week, time of day) are derived from `now_s`, never stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    /// Current campaign time in seconds.
    pub now_s: CampaignTime,
}

/// Campaign-wide settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignSettings {
    /// Default audience for player rolls; default `RollerAndDm`.
    pub player_roll_audience_default: Audience,
    /// Ring buffer capacity of the feed.
    pub feed_capacity: u32,
    /// Hour of day (0 to 23) at which Dawn recharges happen.
    pub dawn_hour: u8,
}

/// The initiative order for the current encounter.
///
/// A plain, DM-owned list: the engine does not roll initiative or model
/// actions. Entries stay in turn order (descending value, ties keeping their
/// previous relative order) and `current` names the participant whose turn it
/// is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Initiative {
    /// Participants in turn order.
    #[serde(default)]
    pub entries: Vec<InitiativeEntry>,
    /// The participant whose turn it is; absent while the encounter waits to
    /// start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<EntityId>,
    /// The current round; `0` while the encounter has not started.
    #[serde(default)]
    pub round: u32,
}

impl Initiative {
    /// True when no participant is tracked.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One participant in the initiative order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitiativeEntry {
    /// The participating entity.
    pub entity: EntityId,
    /// Name snapshot taken when the entry was created.
    pub name: String,
    /// Entity owner snapshot; the browser uses it to show a player their row.
    pub owner: PlayerId,
    /// The rolled or entered initiative value.
    pub value: i32,
}

/// A campaign participant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    /// Player id.
    pub id: PlayerId,
    /// Display name.
    pub display_name: String,
    /// DM or player role.
    pub role: Role,
    /// Hex-encoded digest of the join token; never the token itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_hash: Option<TokenHash>,
    /// When the player joined.
    pub created_at: UnixTime,
    /// Last time the player was seen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<UnixTime>,
}

/// Lowercase-hex digest of a join token.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenHash(String);

impl TokenHash {
    /// The hex length of a SHA-256 digest.
    pub const SHA256_HEX_LEN: usize = 64;

    /// Parses a non-empty lowercase-hex digest.
    ///
    /// The digest length is not fixed, so the hash algorithm can change without
    /// touching the data model.
    #[inline]
    pub fn parse(raw: &str) -> Result<Self, TokenHashError> {
        if raw.is_empty()
            || !raw
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(TokenHashError::Invalid);
        }
        Ok(Self(raw.to_owned()))
    }

    /// True when the digest is a full-length SHA-256 hash.
    #[must_use]
    #[inline]
    pub fn is_sha256(&self) -> bool {
        self.0.len() == Self::SHA256_HEX_LEN
    }

    /// The digest as a lowercase-hex string.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for TokenHash {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TokenHash {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Campaign role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Dungeon master.
    Dm,
    /// Player.
    Player,
}
