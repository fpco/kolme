use anyhow::Result;
use kolme::{ExecutionContext, KolmeApp};
mod serializers;

#[derive(Clone)]
struct VersionUpgradeTestApp {}

#[derive(Clone, Debug)]
struct VersionUpgradeTestState {}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct VersionUpgradeTestMessage {}

impl KolmeApp for VersionUpgradeTestApp {
    type State = VersionUpgradeTestState;

    type Message = VersionUpgradeTestMessage;

    fn genesis_info(&self) -> &kolme::GenesisInfo {
        todo!()
    }

    fn new_state() -> Result<Self::State> {
        Ok(Self::State {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> Result<()> {
        Ok(())
    }
}

pub async fn processor() {
    println!("I'm a processor, yay!")
}
