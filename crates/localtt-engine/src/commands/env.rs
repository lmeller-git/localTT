//! The environment a command may consult besides the working set.

use localtt_core::types::{DiceResult, UnixTime};

use crate::{
    content::{EmptyReference, ReferenceSource},
    dice::RollPlan,
    error::EngineError,
};

/// Everything a command may consult that is not the working set.
///
/// `reference` and `roll` are defaulted (see `docs/engine.md` section 4): a test
/// environment over the defaults fails closed on dice and sees an empty
/// content source, so the frozen U3 envs compile unchanged.
pub trait CommandEnv: Send {
    /// Wall-clock time for `updated_at` and feed `ts` stamps.
    fn now(&self) -> UnixTime;

    /// Read-only content access.
    ///
    /// The default is an empty source: `get` answers `Ok(None)`, search and
    /// autocomplete answer no hits, `sources` is empty.
    #[inline]
    fn reference(&self) -> &dyn ReferenceSource {
        &EmptyReference
    }

    /// Rolls one plan with the environment's die source.
    ///
    /// The default fails with `Environment`: a command cannot conjure an
    /// RNG, and a missing port is an environment fault, not bad input.
    #[inline]
    fn roll(&mut self, _plan: &RollPlan) -> Result<DiceResult, EngineError> {
        Err(EngineError::Environment {
            reason: "no dice source is configured".to_owned(),
        })
    }
}
