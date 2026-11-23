//! Conversions between the engine's `outcome` types and their DTOs.

use localtt_engine::executor::Executed;

use crate::{EventDto, ExecutedDto, WarningDto};

impl From<Executed> for ExecutedDto {
    #[inline]
    fn from(executed: Executed) -> Self {
        Self {
            warnings: executed
                .warnings
                .into_iter()
                .map(WarningDto::from)
                .collect(),
            events: executed.events.into_iter().map(EventDto::from).collect(),
        }
    }
}
