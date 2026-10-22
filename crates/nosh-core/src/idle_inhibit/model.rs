/// The idle inhibitor state published to the UI.
///
/// `available` is true only while the compositor advertises
/// `zwp_idle_inhibit_manager_v1` and the inhibitor surface exists. `active` is
/// true only while an inhibitor is attached to that surface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdleInhibitState {
    pub available: bool,
    pub active: bool,
}

impl IdleInhibitState {
    pub fn new(available: bool, active: bool) -> Self {
        Self { available, active }
    }
}
