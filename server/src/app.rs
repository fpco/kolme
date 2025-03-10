use std::borrow::Cow;

use crate::framework_state::RawFrameworkState;

pub trait KolmeApp {
    fn initial_framework_state() -> RawFrameworkState;
    fn kolme_ident() -> Cow<'static, str> {
        Self::initial_framework_state().kolme_ident.into()
    }
}
