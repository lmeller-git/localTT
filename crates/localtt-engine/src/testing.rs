//! Test support for the contract suites; compiled only for the crate's own
//! tests and behind the `testing` feature.

use localtt_core::types::PlayerId;

use crate::{
    commands::{Command, CommandEnv},
    error::EngineError,
    executor::{Executed, Executor},
    ports::store::CampaignStore,
    state::{WorkingSet, WorkingSetMut},
};

/// An [`Executor`] bundled with the store it writes to.
///
/// The production executor takes its store per call. The contract suites run
/// hundreds of commands against one store and read the result back, so this
/// holds the pair and keeps their call sites short. It adds no behaviour: every
/// method forwards to the executor with the bound store.
pub struct BoundExecutor<'a> {
    executor: Executor,
    store: &'a mut dyn CampaignStore,
}

impl<'a> BoundExecutor<'a> {
    /// Binds a new executor over `state` and `env` to `store`.
    #[must_use]
    #[inline]
    pub fn new(
        state: Box<dyn WorkingSetMut>,
        store: &'a mut dyn CampaignStore,
        env: Box<dyn CommandEnv>,
    ) -> Self {
        Self {
            executor: Executor::new(state, env),
            store,
        }
    }

    /// The visible state.
    #[must_use]
    #[inline]
    pub fn state(&self) -> &dyn WorkingSet {
        self.executor.state()
    }

    /// [`Executor::execute`] against the bound store.
    ///
    /// # Errors
    ///
    /// As [`Executor::execute`].
    #[inline]
    pub fn execute(&mut self, command: &Command) -> Result<Executed, EngineError> {
        self.executor.execute(self.store, command)
    }

    /// [`Executor::execute_as`] against the bound store.
    ///
    /// # Errors
    ///
    /// As [`Executor::execute_as`].
    #[inline]
    pub fn execute_as(
        &mut self,
        actor: PlayerId,
        command: &Command,
    ) -> Result<Executed, EngineError> {
        self.executor.execute_as(self.store, actor, command)
    }
}
