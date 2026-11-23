//! The environment a command may consult besides the working set.

use localtt_core::types::{DiceResult, Id, UnixTime, Uuid};

use crate::{content::ReferenceSource, dice::RollPlan, error::EngineError};

/// Everything a command may consult that is not the working set: the wall
/// clock, the content source, the die source and the id source.
///
/// Every method is required, so an environment that forgets a capability fails
/// to compile instead of failing at run time. An environment with no dice
/// answers [`CommandEnv::roll`] with [`EngineError::Environment`]; a
/// production environment draws from its RNG.
pub trait CommandEnv: Send {
    /// Wall-clock time for `updated_at` and feed `ts` stamps.
    fn now(&self) -> UnixTime;

    /// Read-only content access.
    fn reference(&self) -> &dyn ReferenceSource;

    /// Rolls one plan with the environment's die source.
    ///
    /// # Errors
    ///
    /// [`EngineError::Environment`] when the environment has no die source: a
    /// command cannot conjure an RNG, and a missing port is an environment
    /// fault, not bad input.
    fn roll(&mut self, plan: &RollPlan) -> Result<DiceResult, EngineError>;

    /// Mints the next identifier for an element or document a command creates.
    ///
    /// Production environments return a UUID v7, so ids sort by creation time
    /// (upserts and feed projections are ordered by id). A test environment may
    /// return a deterministic ascending sequence to make a command replayable.
    /// Every value must be unique within the campaign.
    fn new_uuid(&mut self) -> Uuid;
}

impl dyn CommandEnv + '_ {
    /// Mints the next typed identifier from [`CommandEnv::new_uuid`].
    #[must_use]
    #[inline]
    pub fn new_id<T>(&mut self) -> Id<T> {
        Id::from_uuid(self.new_uuid())
    }
}
