/// Internal Kolme state management.
///
/// We split up the state of any Kolme application into three distinct pieces:
///
/// * Event metadata: handles things like account IDs, nonce management, etc. These are pieces of state necessary for managing the event stream itself.
///
/// * Framework state: manages framework data like account balances which are modified during execution. Since some events will fail, we can easily have an event that updates event metadata but has no impact on the framework state.
///
/// * App specific state: storage defined by each application, also updated only during execution.
use std::collections::HashMap;

use crate::*;

mod event;
mod framework;

pub(crate) struct KolmeState<App: KolmeApp> {
    pub event: event::EventState,
    pub framework: framework::FrameworkState,
    pub app: App::State,
    /// Serialized format stored to quickly reload if we need to roll back event processing.
    pub app_serialized: Vec<u8>,
}

impl<App: KolmeApp> KolmeState<App> {
    pub async fn load(
        _app: &App,
        last_event_height: EventHeight,
        last_state_height: EventHeight,
        payload: &[u8],
    ) -> Result<Self> {
        let raw = serde_json::from_slice::<RawFrameworkState>(payload)?;
        anyhow::ensure!(raw.kolme_ident == App::kolme_ident());
        Self::from_raw(raw, last_event_height.next(), last_state_height.next())
    }

    pub(crate) fn new(
        last_event_height: Option<EventHeight>,
        raw: RawFrameworkState,
    ) -> Result<Self> {
        Self::from_raw(
            raw,
            last_event_height.map_or_else(EventHeight::start, EventHeight::next),
            EventHeight::start(),
        )
    }

    fn from_raw(
        raw: RawFrameworkState,
        next_event_height: EventHeight,
        next_state_height: EventHeight,
    ) -> Result<FrameworkState> {
        anyhow::ensure!(next_event_height >= next_state_height);
        raw.validate()?;

        Ok(FrameworkState {
            raw,
            next_event_height,
            next_state_height,
            pubkeys,
            wallets,
        })
    }

    pub(crate) fn validate_code_version(&self, code_version: impl AsRef<str>) -> Result<()> {
        let code_version = code_version.as_ref();
        anyhow::ensure!(
            self.raw.code_version == code_version,
            "Could not match discovered code version {} against actual code version {}",
            self.raw.code_version,
            code_version
        );
        Ok(())
    }
}
