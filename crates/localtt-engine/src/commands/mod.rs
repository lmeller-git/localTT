//! The command surface (RFC-0001 5.2): the mutating operations a client
//! drives.
//!
//! The U3 slice (see `docs/engine.md` section 2) is clock, statuses, hp,
//! resources and rests; U5 adds dice, casting and hit dice (see
//! `docs/engine.md` section 4); later units add inventory, entities and seats.
//! Spell and resource CRUD (`AddSpell`/`EditSpell`/`RemoveSpell` and
//! `AddResource`/`EditResource`/`RemoveResource`) back the paper-sheet spell
//! list (see `docs/engine.md` section 4.5). The free helper functions of the
//! pre-U3 surface and the unauthenticated `apply` exist only behind the
//! `testing` feature; commands reach the working set through the executor
//! (`docs/engine.md` section 2).

mod casting;
mod clock;
mod command;
mod creation;
mod dice_cmd;
mod dispatch;
#[cfg(any(test, feature = "testing"))]
mod entities;
mod env;
mod hp;
mod initiative;
mod inventory;
mod players;
mod resources;
mod rests;
mod spells;
mod statuses;
mod support;
mod trading;
mod transition;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testing"))]
pub(crate) use self::hp::{apply_damage, set_temp_hp};
use self::support::{
    campaign_change,
    cap_feed,
    load_statblock,
    load_target,
    one_upsert,
    stamp,
    stamped_with_feed,
    two_upserts,
};
pub use self::{
    command::{
        Command,
        ItemMove,
        MAX_DISPLAY_NAME,
        MAX_PLAYERS,
        NewSpell,
        RollSpec,
        SpellPlacement,
        StatusSpec,
    },
    env::CommandEnv,
    transition::{Event, Transition},
};
#[cfg(any(test, feature = "testing"))]
pub use self::{
    dispatch::apply,
    entities::{level_up, summon_from_template},
    inventory::give_handout,
    statuses::begin_concentration,
};
pub(crate) use self::{dispatch::apply_as, hp::heal, resources::spend_resource};
