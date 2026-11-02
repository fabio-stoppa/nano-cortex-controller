#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Searching,
    Connecting,
    Discovering,
    Subscribing,
    Loading,
    Ready,
}

impl Stage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Searching => "Searching",
            Self::Connecting => "Connecting",
            Self::Discovering => "Discovering",
            Self::Subscribing => "Subscribing",
            Self::Loading => "Loading",
            Self::Ready => "Ready",
        }
    }
}
