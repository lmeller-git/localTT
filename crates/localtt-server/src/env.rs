//! The process environment a command sees (`docs/rfcs/server.md` 4.2, F5).

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use localtt_adapter::rng::ChaChaRng;
use localtt_core::types::{DiceResult, UnixTime};
use localtt_engine::{
    commands::CommandEnv,
    content::ReferenceSource,
    dice::{self, RollPlan},
    error::EngineError,
};

/// Wall clock, one seeded die source and the shared reference source.
///
/// The environment is owned by the actor between requests. The RNG lives here
/// so a recovered executor keeps drawing from the same sequence; the reference
/// source is shared with the content read path.
pub struct ServerEnv {
    rng: ChaChaRng,
    reference: Arc<dyn ReferenceSource>,
}

impl ServerEnv {
    /// Builds the environment with an OS-seeded die source.
    ///
    /// # Panics
    ///
    /// Panics when the OS entropy source is unavailable; there is no
    /// meaningful fallback seed.
    #[must_use]
    #[inline]
    pub fn new(reference: Arc<dyn ReferenceSource>) -> Self {
        Self {
            rng: ChaChaRng::new(),
            reference,
        }
    }

    /// Builds the environment with an explicit die source.
    ///
    /// Tests use a seeded source so a session can be replayed.
    #[must_use]
    #[inline]
    pub fn with_rng(reference: Arc<dyn ReferenceSource>, rng: ChaChaRng) -> Self {
        Self { rng, reference }
    }
}

impl CommandEnv for ServerEnv {
    #[inline]
    fn now(&self) -> UnixTime {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        UnixTime::from_seconds(i64::try_from(seconds).unwrap_or(i64::MAX))
    }

    #[inline]
    fn reference(&self) -> &dyn ReferenceSource {
        self.reference.as_ref()
    }

    #[inline]
    fn roll(&mut self, plan: &RollPlan) -> Result<DiceResult, EngineError> {
        Ok(dice::roll(plan, &mut self.rng))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use localtt_adapter::reference::FixtureSource;
    use localtt_engine::{
        commands::CommandEnv,
        dice,
        rules::{RollHint, RollParts},
    };

    use super::ServerEnv;

    #[test]
    fn now_reads_the_wall_clock() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the test clock is after the epoch")
            .as_secs();
        let env = ServerEnv::with_rng(
            Arc::new(FixtureSource::new()),
            localtt_adapter::rng::ChaChaRng::from_seed([3; 32]),
        );
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the test clock is after the epoch")
            .as_secs();

        let now = u64::try_from(env.now().seconds()).expect("the wall clock is positive");
        assert!((before..=after).contains(&now));
    }

    #[test]
    fn rolls_use_the_configured_source() {
        let mut env = ServerEnv::with_rng(
            Arc::new(FixtureSource::new()),
            localtt_adapter::rng::ChaChaRng::from_seed([9; 32]),
        );
        let expression = dice::parse("1d6").expect("a literal expression");
        let plan = dice::compose(
            expression,
            &RollParts {
                flat: 0,
                dice: Vec::new(),
                hint: RollHint::Normal,
            },
        );

        let result = env.roll(&plan).expect("a configured source rolls");
        assert_eq!(result.terms.len(), 1);
        assert_eq!(result.terms[0].dice.len(), 1);
        assert!((1..=6).contains(&result.terms[0].dice[0].value));
    }

    #[test]
    fn the_reference_source_is_the_shared_one() {
        let source = Arc::new(FixtureSource::new());
        let env = ServerEnv::new(source);
        assert_eq!(env.reference().sources().len(), 1);
    }
}
