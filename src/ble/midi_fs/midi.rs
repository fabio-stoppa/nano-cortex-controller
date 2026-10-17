//! BLE-MIDI packet parser (timestamp-header framing used by M-Vave Chocolate).
//!
//! Adapted from Builty TonexOneController `midi_helper_process_incoming_data`.

#[derive(Copy, Clone, Debug)]
pub(super) enum MidiMsg {
    /// Control Change: controller number + value (0..=127).
    Cc { channel: u8, cc: u8, value: u8 },
    /// Program Change (ignored for FX mapping v1 — logged only).
    Pc { channel: u8, program: u8 },
}

/// Decode one BLE-MIDI ATT notification into zero or more MIDI messages.
pub(super) fn parse_ble_midi(data: &[u8], out: &mut [MidiMsg; 8]) -> usize {
    if data.len() < 2 {
        return 0;
    }

    let mut i = 0usize;
    let mut n = 0usize;

    // Optional header: 0b10xxxxxx followed by a byte with bit7 set.
    let header_found = (data[0] & 0x80) != 0 && (data[0] & 0x40) == 0 && (data[1] & 0x80) != 0;
    if header_found {
        i = 1;
    }

    while i < data.len() && n < out.len() {
        if header_found && (data[i] & 0x80) != 0 {
            // Timestamp LSB — skip.
            i += 1;
            if i >= data.len() {
                break;
            }
        }

        let status = data[i];
        if (status & 0x80) == 0 {
            // Running status / unexpected data — bail.
            break;
        }
        i += 1;

        let channel = status & 0x0F;
        let command = status & 0xF0;

        match command {
            0xC0 => {
                if i >= data.len() {
                    break;
                }
                let program = data[i] & 0x7F;
                i += 1;
                out[n] = MidiMsg::Pc { channel, program };
                n += 1;
            }
            0xB0 => {
                // One or more CC pairs until a high-bit timestamp/status.
                while i + 1 < data.len() && n < out.len() {
                    if (data[i] & 0x80) != 0 {
                        break;
                    }
                    let cc = data[i] & 0x7F;
                    let value = data[i + 1] & 0x7F;
                    i += 2;
                    out[n] = MidiMsg::Cc {
                        channel,
                        cc,
                        value,
                    };
                    n += 1;
                }
            }
            _ => {
                // Skip unknown status; try to resync on next high bit.
                break;
            }
        }
    }

    n
}
