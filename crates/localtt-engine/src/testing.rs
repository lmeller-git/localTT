//! Test support for the contract suites; compiled only for the crate's own
//! tests and behind the `testing` feature.
//!
//! Besides [`BoundExecutor`] and the helpers below, the feature exposes the
//! unauthenticated paths the suites drive directly: `Executor::plan`,
//! `Executor::execute`, `commands::apply` and the pre-command helper functions
//! (`level_up`, `summon_from_template`, `give_handout`,
//! `begin_concentration`). None of them is part of the production surface.

use localtt_core::types::{PlayerId, ResourceId, StatblockEntity};

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

/// Applies `amount` damage to a bare statblock: temp HP absorbs first, then
/// current; `current` may go negative. The arithmetic saturates.
#[inline]
pub fn apply_damage(entity: &mut StatblockEntity, amount: i32) {
    crate::commands::apply_damage(entity, amount);
}

/// Heals `amount` on a bare statblock; temp HP is not restored and the result
/// clamps to `max`.
#[inline]
pub fn heal(entity: &mut StatblockEntity, amount: i32) {
    crate::commands::heal(entity, amount);
}

/// Sets temp HP on a bare statblock; non-stacking (keeps the higher value).
#[inline]
pub fn set_temp_hp(entity: &mut StatblockEntity, amount: i32) {
    crate::commands::set_temp_hp(entity, amount);
}

/// Spends `amount` of a resource on a bare statblock; overspending is allowed
/// and the arithmetic saturates.
#[inline]
pub fn spend_resource(entity: &mut StatblockEntity, id: ResourceId, amount: i32) {
    crate::commands::spend_resource(entity, id, amount);
}
