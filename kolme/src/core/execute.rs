use crate::core::*;

/// Execution context for a single message.
pub struct ExecutionContext<App: KolmeApp> {
    framework_state: FrameworkState,
    app_state: App::State,
    output: MessageOutput,
}

pub(crate) struct ExecutionResults<App: KolmeApp> {
    pub(crate) framework_state: FrameworkState,
    pub(crate) app_state: App::State,
    pub(crate) outputs: Vec<MessageOutput>,
}

impl<App: KolmeApp> ExecutionContext<App> {
    pub fn consume(self) -> (FrameworkState, App::State, MessageOutput) {
        (self.framework_state, self.app_state, self.output)
    }
}

impl<App: KolmeApp> KolmeInner<App> {
    pub async fn execute_messages(
        &self,
        messages: &[Message<App::Message>],
    ) -> Result<ExecutionResults<App>> {
        let mut outputs = vec![];
        let mut execution_context = ExecutionContext::<App> {
            framework_state: self.framework_state.clone(),
            app_state: self.app_state.clone(),
            output: MessageOutput::default(),
        };
        for message in messages {
            execution_context.execute_message(message).await?;
            let output = std::mem::take(&mut execution_context.output);
            outputs.push(output);
        }
        Ok(ExecutionResults {
            framework_state: execution_context.framework_state,
            app_state: execution_context.app_state,
            outputs,
        })
    }
}

impl<App: KolmeApp> ExecutionContext<App> {
    async fn execute_message(&mut self, message: &Message<App::Message>) -> Result<()> {
        match message {
            Message::Genesis(_genesis_info) => {
                // FIXME We could do some sanity checks that the genesis info lines up with
                // the stored state, but just trusting the system for now.
            }
            // FIXME BridgeCreated should be part of the listener messages, and we should have a BridgeCreated Notification that the listeners wait for
            Message::BridgeCreated(BridgeCreated { chain, contract }) => {
                todo!()
                // context.state.state.exec.bridge_created(*chain, contract)?;
            }
            Message::App(_) => todo!(),
            Message::Listener(_listener_message) => todo!(),
            Message::Auth(_auth_message) => todo!(),
        }
        Ok(())
    }
}
