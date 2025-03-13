use crate::core::*;

/// Execution context for a single message.
pub struct ExecutionContext<'a, App: KolmeApp> {
    state: &'a mut KolmeState<App>,
    output: MessageOutput,
}

impl<App: KolmeApp> KolmeState<App> {
    pub(crate) async fn execute_messages(
        &mut self,
        messages: &[EventMessage<App::Message>],
    ) -> Result<Vec<MessageOutput>> {
        let mut ret = vec![];
        for message in messages {
            let output = self.execute_message(message).await?;
            ret.push(output);
        }
        Ok(ret)
    }

    async fn execute_message(
        &mut self,
        message: &EventMessage<App::Message>,
    ) -> Result<MessageOutput> {
        let context = ExecutionContext {
            state: self,
            output: MessageOutput::default(),
        };
        match message {
            EventMessage::Genesis(_genesis_info) => {
                // We could do some sanity checks that the genesis info lines up with
                // the stored state, but just trusting the system for now.
            }
            EventMessage::App(_) => todo!(),
            EventMessage::Listener(_listener_message) => todo!(),
            EventMessage::Auth(_auth_message) => todo!(),
        }
        Ok(context.output)
    }
}
