use k256::ecdsa::Signature;

use crate::*;

/// An executed event that is signed by the processor.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SignedExec {
    /// An [ExecutedEvent], see [SignedEvent::event] for details.
    pub exec: TaggedJson<ExecutedEvent>,
    pub signature: Signature,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExecutedEvent {
    pub height: EventHeight,
    pub timestamp: Timestamp,
    // FIXME pub event_hash: EventHash
    // pub previous_executed_event: Option<ExecHash>
    pub framework_state: Sha256Hash,
    pub app_state: Sha256Hash,
    pub loads: Vec<ExecLoad>,
}

/// Execution context for a single message.
pub struct ExecutionContext<'a, App: KolmeApp> {
    state: &'a mut KolmeState<App>,
    output: MessageOutput,
}

#[derive(Default)]
pub(crate) struct MessageOutput {
    pub(crate) logs: Vec<String>,
    pub(crate) loads: Vec<ExecLoad>,
    pub(crate) actions: Vec<ExecAction>,
}

/// Input and output for a single data load.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExecLoad {
    /// Description of the query
    pub query: String,
    /// The resulting value
    pub response: String,
}

/// A specific action to be taken as a result of an execution.
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ExecAction {
    Transfer {
        recipient: String,
        funds: Vec<AssetAmount>,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AssetAmount {
    pub id: AssetId,
    pub amount: u128, // FIXME use a Decimal representation
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
