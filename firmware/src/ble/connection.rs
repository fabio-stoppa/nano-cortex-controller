use core::future::Future;
use embassy_time::{with_timeout, Duration};

pub const SETUP_TIMEOUT: Duration = Duration::from_secs(8);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, PartialEq, Eq)]
pub enum Failure<E> {
    Bluetooth(E),
    Timeout,
}

pub async fn with_deadline<T, E>(
    deadline: Duration,
    operation: impl Future<Output = Result<T, E>>,
) -> Result<T, Failure<E>> {
    match with_timeout(deadline, operation).await {
        Ok(result) => result.map_err(Failure::Bluetooth),
        Err(_) => Err(Failure::Timeout),
    }
}

pub async fn run_io(receive: impl Future<Output = ()>, commands: impl Future<Output = ()>) {
    embassy_futures::select::select(receive, commands).await;
}

pub const TOUCH_CONTROL_COUNT: usize = 8;
pub const MAX_RECONNECT_BACKOFF_SECS: u8 = 30;

pub const fn next_reconnect_backoff_secs(current: u8) -> u8 {
    if current >= MAX_RECONNECT_BACKOFF_SECS / 2 {
        MAX_RECONNECT_BACKOFF_SECS
    } else {
        current * 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliderWrite {
    pub id: u8,
    pub raw: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchEdits {
    pending: [Option<SliderWrite>; TOUCH_CONTROL_COUNT],
}

impl TouchEdits {
    pub const fn new() -> Self {
        Self {
            pending: [None; TOUCH_CONTROL_COUNT],
        }
    }

    pub fn push(&mut self, edit: SliderWrite) -> bool {
        if let Some(slot) = self
            .pending
            .iter_mut()
            .find(|slot| slot.is_some_and(|pending| pending.id == edit.id))
        {
            *slot = Some(edit);
            return true;
        }
        if let Some(slot) = self.pending.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(edit);
            return true;
        }
        false
    }

    pub fn pop(&mut self) -> Option<SliderWrite> {
        let slot = self.pending.iter_mut().find(|slot| slot.is_some())?;
        slot.take()
    }

    pub fn clear(&mut self) {
        self.pending.fill(None);
    }
}

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

#[derive(Clone, Copy, Debug)]
pub enum Operation {
    Connect,
    DataLength,
    Gatt,
    Services,
    Characteristics,
    Subscribe,
    Write,
    Loading,
    Receive,
    Scan,
}

impl Operation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connect => "Nano connection",
            Self::DataLength => "Nano data length setup",
            Self::Gatt => "Nano GATT initialization",
            Self::Services => "Nano service discovery",
            Self::Characteristics => "Nano characteristic discovery",
            Self::Subscribe => "C305 subscription",
            Self::Write => "C304 write",
            Self::Loading => "Nano initial loading",
            Self::Receive => "Nano receive",
            Self::Scan => "Nano scan",
        }
    }

    pub const fn recovery(self) -> &'static str {
        match self {
            Self::Connect | Self::Scan => {
                "Move Nano closer, enable pairing, and disconnect Cortex Cloud."
            }
            Self::DataLength | Self::Gatt | Self::Receive => {
                "Check Nano power/range; restart the controller if this repeats."
            }
            Self::Services | Self::Characteristics => {
                "Check that this is a Nano Cortex with compatible firmware."
            }
            Self::Subscribe => "Check Nano pairing; authentication errors may require re-pairing.",
            Self::Write => {
                "Check the sound before retrying; save/copy/move/delete may have applied."
            }
            Self::Loading => "Keep Nano nearby and disconnect other clients; loading will restart.",
        }
    }
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
        for bits in 0..16 {
            assert_eq!(
                validate_capabilities(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0,)
                    .is_ok(),
                bits == 15,
            );
        }
    }

    #[test]
    fn touch_edits_coalesce_per_control_without_losing_other_controls() {
        let mut edits = TouchEdits::new();
        assert!(edits.push(SliderWrite { id: 1, raw: 10 }));
        assert!(edits.push(SliderWrite { id: 2, raw: 20 }));
        assert!(edits.push(SliderWrite { id: 1, raw: 99 }));
        assert_eq!(edits.pop(), Some(SliderWrite { id: 1, raw: 99 }));
        assert_eq!(edits.pop(), Some(SliderWrite { id: 2, raw: 20 }));
        assert_eq!(edits.pop(), None);
    }

    #[test]
    fn full_touch_queue_rejects_new_control_but_replaces_queued_value() {
        let mut edits = TouchEdits::new();
        for id in 0..TOUCH_CONTROL_COUNT as u8 {
            assert!(edits.push(SliderWrite { id, raw: 0 }));
        }
        assert!(!edits.push(SliderWrite { id: 8, raw: 1 }));
        assert!(edits.push(SliderWrite { id: 3, raw: 77 }));
        assert_eq!(edits.pop(), Some(SliderWrite { id: 3, raw: 77 }));
        edits.clear();
        assert_eq!(edits.pop(), None);
    }

    #[test]
    fn reconnect_backoff_doubles_and_caps() {
        let mut delay = 1;
        let mut seen = [0; 6];
        for value in &mut seen {
            *value = delay;
            delay = next_reconnect_backoff_secs(delay);
        }
        assert_eq!(seen, [1, 2, 4, 8, 16, 30]);
        assert_eq!(next_reconnect_backoff_secs(30), 30);
    }
}
