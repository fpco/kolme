use crate::core::*;

/// Execution context for a single message.
pub struct ExecutionContext<App: KolmeApp> {
    framework_state: FrameworkState,
    app_state: App::State,
    output: MessageOutput,
    /// If we're doing a validation run, these are the prior data loads.
    validation_data_loads: Option<Vec<BlockDataLoad>>,
}

pub struct ExecutionResults<App: KolmeApp> {
    pub framework_state: FrameworkState,
    pub app_state: App::State,
    pub outputs: Vec<MessageOutput>,
}

impl<App: KolmeApp> KolmeInner<App> {
    /// Provide the validation data loads if we're doing a validation of a block.
    pub async fn execute_messages(
        &self,
        messages: &[Message<App::Message>],
        validation_data_loads: Option<Vec<BlockDataLoad>>,
    ) -> Result<ExecutionResults<App>> {
        let mut outputs = vec![];
        let mut execution_context = ExecutionContext::<App> {
            framework_state: self.framework_state.clone(),
            app_state: self.app_state.clone(),
            output: MessageOutput::default(),
            validation_data_loads,
        };
        for message in messages {
            execution_context.execute_message(message).await?;
            let output = std::mem::take(&mut execution_context.output);
            outputs.push(output);
        }
        if let Some(loads) = execution_context.validation_data_loads {
            // For a proper validation, every piece of data loaded during execution
            // must be used during validation.
            anyhow::ensure!(loads.is_empty());
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
            Message::App(_) => todo!(),
            Message::Listener { chain, event } => todo!(),
            Message::Auth(_auth_message) => todo!(),
        }
        Ok(())
    }
}
