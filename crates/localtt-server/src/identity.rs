//! Roster-derived bearer-token index (M5).
//!
//! The index is the single authentication source: a full SHA-256 token hash
//! resolves to exactly one roster seat. Legacy or malformed short hashes are
//! skipped, so they can never authenticate.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use localtt_core::types::{Campaign, PlayerId, Role, TokenHash};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// The identity a bearer token resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewer {
    /// The roster seat.
    pub id: PlayerId,
    /// The seat's role.
    pub role: Role,
}

/// A bearer token's resolved viewer and the digest that resolved it.
///
/// The digest, not the raw token, is kept so a long-lived stream can be
/// revalidated after a roster commit.
#[derive(Clone, Debug)]
pub(crate) struct Credential {
    /// The identity the token resolved to.
    pub(crate) viewer: Viewer,
    /// The SHA-256 digest of the raw bearer token.
    pub(crate) token_hash: TokenHash,
}

/// Why a roster cannot produce an auth index.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthIndexError {
    /// Two seats share an id.
    #[error("player {player} appears twice on the roster")]
    DuplicatePlayer {
        /// The duplicated seat id.
        player: PlayerId,
    },
    /// Two seats share a full SHA-256 token hash.
    #[error("the same token hash is assigned to players {first} and {second}")]
    DuplicateHash {
        /// The seat that already owned the hash.
        first: PlayerId,
        /// The second seat claiming it.
        second: PlayerId,
    },
}

/// A lookup from full SHA-256 token hashes to roster seats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthIndex {
    seats: BTreeMap<TokenHash, Viewer>,
}

impl AuthIndex {
    /// Builds the index from a campaign roster.
    ///
    /// A seat without a hash, or with a hash that is not a full SHA-256 digest,
    /// is left out and logged as a warning. Duplicate ids and duplicate
    /// full-length hashes fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`AuthIndexError::DuplicatePlayer`] for a repeated seat id and
    /// [`AuthIndexError::DuplicateHash`] when two seats claim one hash.
    #[inline]
    pub fn build(campaign: &Campaign) -> Result<Self, AuthIndexError> {
        let mut ids = BTreeSet::new();
        let mut seats = BTreeMap::new();
        for player in &campaign.players {
            if !ids.insert(player.id) {
                return Err(AuthIndexError::DuplicatePlayer { player: player.id });
            }
            let Some(hash) = player.token_hash.as_ref().filter(|hash| hash.is_sha256()) else {
                if player.token_hash.is_some() {
                    tracing::warn!(
                        player = %player.id,
                        "skipping a token hash that is not a full SHA-256 digest"
                    );
                }
                continue;
            };
            let viewer = Viewer {
                id: player.id,
                role: player.role,
            };
            if let Some(previous) = seats.insert(hash.clone(), viewer) {
                return Err(AuthIndexError::DuplicateHash {
                    first: previous.id,
                    second: player.id,
                });
            }
        }
        Ok(Self { seats })
    }

    /// Resolves a full token hash.
    #[must_use]
    #[inline]
    pub fn lookup(&self, hash: &TokenHash) -> Option<Viewer> {
        self.seats.get(hash).copied()
    }

    /// Hashes a raw bearer token and resolves it.
    #[must_use]
    #[inline]
    pub fn authenticate(&self, token: &str) -> Option<Viewer> {
        self.lookup(&hash_token(token))
    }

    /// The number of indexed seats.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.seats.len()
    }

    /// True when no seat has a usable token.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }
}

/// SHA-256 of a raw token as a lowercase-hex digest.
#[must_use]
#[inline]
pub fn hash_token(token: &str) -> TokenHash {
    let digest = Sha256::digest(token.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    TokenHash::parse(&hex).expect("a SHA-256 digest is 64 lowercase hex characters")
}

#[cfg(test)]
mod tests {
    use localtt_core::{
        builders::CampaignBuilder,
        types::{Player, UnixTime},
    };

    use super::*;

    fn campaign() -> (Campaign, PlayerId, PlayerId) {
        let mut builder = CampaignBuilder::new("Curse of the Vault");
        let dm = builder.dm("DM");
        let player = builder.player("Alex");
        (builder.build(), dm, player)
    }

    fn assign(campaign: &mut Campaign, player: PlayerId, token: &str) {
        campaign
            .players
            .iter_mut()
            .find(|seat| seat.id == player)
            .expect("the seat exists")
            .token_hash = Some(hash_token(token));
    }

    #[test]
    fn hash_token_matches_a_known_sha256_vector() {
        assert_eq!(
            hash_token("abc").as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn build_resolves_each_role_and_skips_short_hashes() {
        // Arrange
        let (mut campaign, dm, player) = campaign();
        assign(&mut campaign, dm, "dm-secret");
        assign(&mut campaign, player, "player-secret");
        let legacy = PlayerId::new();
        campaign.players.push(Player {
            id: legacy,
            display_name: "Legacy".to_owned(),
            role: Role::Player,
            token_hash: Some(TokenHash::parse("0a1b").expect("short hex parses")),
            created_at: UnixTime::from_seconds(0),
            last_seen: None,
        });

        // Act
        let index = AuthIndex::build(&campaign).expect("the roster is unambiguous");

        // Assert
        assert_eq!(
            index.authenticate("dm-secret"),
            Some(Viewer {
                id: dm,
                role: Role::Dm
            })
        );
        assert_eq!(
            index.authenticate("player-secret"),
            Some(Viewer {
                id: player,
                role: Role::Player
            })
        );
        assert_eq!(index.authenticate("wrong"), None);
        assert_eq!(index.len(), 2, "the short legacy hash is not indexed");
    }

    #[test]
    fn build_rejects_a_duplicate_player_id() {
        // Arrange
        let (mut campaign, dm, _) = campaign();
        let mut duplicate = campaign
            .players
            .iter()
            .find(|seat| seat.id == dm)
            .expect("the DM exists")
            .clone();
        duplicate.display_name = "Second DM".to_owned();
        campaign.players.push(duplicate);

        // Act
        let result = AuthIndex::build(&campaign);

        // Assert
        assert_eq!(result, Err(AuthIndexError::DuplicatePlayer { player: dm }));
    }

    #[test]
    fn build_rejects_a_duplicate_token_hash() {
        // Arrange
        let (mut campaign, dm, player) = campaign();
        assign(&mut campaign, dm, "shared-secret");
        assign(&mut campaign, player, "shared-secret");

        // Act
        let result = AuthIndex::build(&campaign);

        // Assert
        assert_eq!(
            result,
            Err(AuthIndexError::DuplicateHash {
                first: dm,
                second: player
            })
        );
    }
}
