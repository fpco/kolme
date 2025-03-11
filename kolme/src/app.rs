use std::borrow::Cow;

use crate::framework_state::RawFrameworkState;

pub trait KolmeApp {
    type State: serde::Serialize + serde::de::DeserializeOwned;
    type Message: serde::Serialize + serde::de::DeserializeOwned;

    fn initial_framework_state() -> RawFrameworkState;
    fn kolme_ident() -> Cow<'static, str> {
        Self::initial_framework_state().kolme_ident.into()
    }
}
