#[cfg(not(test))]
use alloc::vec::Vec;
#[cfg(test)]
use std::vec::Vec;

const MAX_MESSAGE_LEN: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentStatus {
    NotFragment,
    Incomplete,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentError {
    TooLarge,
    Allocation,
}

#[derive(Default)]
pub struct C305FragmentReassembler {
    active: bool,
    message: Vec<u8>,
}

impl C305FragmentReassembler {
    pub const fn new() -> Self {
        Self {
            active: false,
            message: Vec::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.message.clear();
    }

    pub fn push(&mut self, packet: &[u8]) -> Result<FragmentStatus, FragmentError> {
        if packet.len() < 2 {
            return Ok(FragmentStatus::NotFragment);
        }
        let flags = packet[1];
        if flags == 0xC0 {
            return Ok(FragmentStatus::NotFragment);
        }

        if flags & 0x40 != 0 {
            self.message.clear();
            self.active = true;
        } else if !self.active {
            return Ok(FragmentStatus::NotFragment);
        }

        let payload = &packet[2..];
        let Some(message_len) = self.message.len().checked_add(payload.len()) else {
            self.reset();
            return Err(FragmentError::TooLarge);
        };
        if message_len > MAX_MESSAGE_LEN {
            self.reset();
            return Err(FragmentError::TooLarge);
        }
        if self.message.try_reserve(payload.len()).is_err() {
            self.reset();
            return Err(FragmentError::Allocation);
        }
        self.message.extend_from_slice(payload);

        if flags & 0x80 != 0 {
            self.active = false;
            Ok(FragmentStatus::Complete)
        } else {
            Ok(FragmentStatus::Incomplete)
        }
    }

    pub fn take_message(&mut self) -> Vec<u8> {
        self.active = false;
        core::mem::take(&mut self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::{C305FragmentReassembler, FragmentStatus};

    #[test]
    fn reassembles_start_continuation_and_end_packets() {
        let mut reassembler = C305FragmentReassembler::new();
        assert_eq!(
            reassembler.push(&[0xF5, 0x41, 0x08, 0x03]).unwrap(),
            FragmentStatus::Incomplete
        );
        assert_eq!(
            reassembler.push(&[0xF5, 0x01, 0x18, 0x01]).unwrap(),
            FragmentStatus::Incomplete
        );
        assert_eq!(
            reassembler
                .push(&[0x04, 0x81, 0x02, 0x00, 0x00, 0x00])
                .unwrap(),
            FragmentStatus::Complete
        );
        assert_eq!(
            reassembler.take_message().as_slice(),
            &[0x08, 0x03, 0x18, 0x01, 0x02, 0x00, 0x00, 0x00]
        );
        assert!(!reassembler.is_active());
    }

    #[test]
    fn ignores_single_packet_messages_without_disturbing_fragment_state() {
        let mut reassembler = C305FragmentReassembler::new();
        assert_eq!(
            reassembler.push(&[0xF5, 0x41, 0x08]).unwrap(),
            FragmentStatus::Incomplete
        );
        assert_eq!(
            reassembler.push(&[0x06, 0xC0, 0x08, 0x03]).unwrap(),
            FragmentStatus::NotFragment
        );
        assert!(reassembler.is_active());
    }
}
