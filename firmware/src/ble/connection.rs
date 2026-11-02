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

pub fn validate_capabilities(
    a002: bool,
    a003: bool,
    write: bool,
    notify: bool,
) -> Result<(), &'static str> {
    if !a002 || !a003 {
        Err("Discovery failed: required Nano services A002/A003 are missing")
    } else if !write {
        Err("Discovery failed: C304 does not support writes with response")
    } else if !notify {
        Err("Subscription failed: C305 does not support notifications")
    } else {
        Ok(())
    }
}

pub fn initial_state_loaded(snapshot: bool, presets: bool, library: bool) -> bool {
    snapshot && presets && library
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_requires_every_initial_component() {
        for bits in 0..8 {
            assert_eq!(
                initial_state_loaded(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0),
                bits == 7,
            );
        }
    }

    #[test]
    fn validate_each_required_capability() {
        assert!(validate_capabilities(true, true, true, true).is_ok());
        assert!(validate_capabilities(false, true, true, true).is_err());
        assert!(validate_capabilities(true, false, true, true).is_err());
        assert!(validate_capabilities(true, true, false, true).is_err());
        assert!(validate_capabilities(true, true, true, false).is_err());
    }
}
