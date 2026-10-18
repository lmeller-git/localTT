//! The single-writer command loop (RFC `engine-executor.md` 4.4, 5.1).

use crate::{
    commands::{Command, CommandEnv, Event},
    error::{EngineError, Warning},
    ports::store::CampaignStore,
    state::{WorkingSet, WorkingSetMut},
};

/// The single-writer command loop. `Send`, deliberately not `Sync`: the
/// server serializes access (mutex or actor thread, U7).
pub struct Executor<'a> {
    state: Box<dyn WorkingSetMut>,
    store: &'a mut dyn CampaignStore,
    env: Box<dyn CommandEnv>,
}

impl<'a> Executor<'a> {
    /// Creates an executor over a working set, a store and an environment.
    #[must_use]
    #[inline]
    pub fn new(
        state: Box<dyn WorkingSetMut>,
        store: &'a mut dyn CampaignStore,
        env: Box<dyn CommandEnv>,
    ) -> Self {
        todo!()
    }

    /// apply -> persist -> publish. Durable before visible (ENG-2): a failed
    /// store write leaves the working set untouched, and events are returned
    /// only after the commit.
    #[inline]
    pub fn execute(&mut self, command: &Command) -> Result<Executed, EngineError> {
        todo!()
    }

    /// The visible state, for services (7.8) and tests.
    #[must_use]
    #[inline]
    pub fn state(&self) -> &dyn WorkingSet {
        todo!()
    }
}

/// A committed command's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executed {
    /// Non-blocking rule outcomes, in the order the rules produced them.
    pub warnings: Vec<Warning>,
    /// The ordered delivery hint for U7 (5.7).
    pub events: Vec<Event>,
}
