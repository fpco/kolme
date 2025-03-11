use event::EventState;
use exec::ExecutionState;

/// Internal Kolme state management.
///
/// We split up the state of any Kolme application into three distinct pieces:
///
/// * Event metadata: handles things like account IDs, nonce management, etc. These are pieces of state necessary for managing the event stream itself.
///
/// * Framework state: manages framework data like account balances which are modified during execution. Since some events will fail, we can easily have an event that updates event metadata but has no impact on the framework state.
///
/// * App specific state: storage defined by each application, also updated only during execution.
use crate::*;

mod event;
mod exec;

pub(crate) struct KolmeState<App: KolmeApp> {
    pub event: EventState,
    pub exec: ExecutionState<App>,
}

impl<App: KolmeApp> KolmeState<App> {
    pub async fn new(
        app: &App,
        event: Option<EventStreamState>,
        exec: Option<ExecutionStreamState>,
        code_version: &str,
    ) -> Result<Self> {
        let info = App::genesis_info();
        let event = match event {
            Some(event) => EventState::load(&info.kolme_ident, event)?,
            None => EventState::new(&info.kolme_ident)?,
        };
        let exec = match exec {
            Some(exec) => ExecutionState::load(app, exec, code_version)?,
            None => ExecutionState::new(code_version, info)?,
        };
        anyhow::ensure!(event.get_next_height() >= exec.get_next_height());
        Ok(KolmeState { event, exec })
    }
}
