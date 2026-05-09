//! BLE protocol definitions for LEGO DUPLO train.
//!
//! Implements the LEGO Wireless Protocol 3.0 for LPF2 hubs.

use uuid::Uuid;

// BLE Service and Characteristic UUIDs
pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x00001623_1212_efde_1623_785feabcd123);
pub const CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0x00001624_1212_efde_1623_785feabcd123);

// Manufacturer ID for filtering
pub const MANUFACTURER_ID: u16 = 0x0397;

// ── LWP 3.0 protocol constants ────────────────────────────────────────────────

/// Hub port identifiers (byte 3 of LWP frames addressing a specific port).
mod port {
    /// Built-in speaker.
    pub const SPEAKER: u8 = 0x01;
    /// Drive motor.
    pub const MOTOR: u8 = 0x32;
    /// Virtual port that aggregates LED and horn sub-commands.
    pub const MULTI: u8 = 0x34;
    /// Speedometer (original train hub).
    pub const SPEEDOMETER: u8 = 0x36;
    /// Speedometer port id reported by newer train hubs.
    pub const SPEEDOMETER_ALT: u8 = 0x33;
    /// `WriteDirectModeData` mode index used when targeting the speaker.
    pub const SPEAKER_MODE: u8 = 0x01;
}

/// Message type bytes (byte 2 of every LWP frame).
mod msg {
    /// Hub property request/response (e.g. battery level).
    pub const PROPERTY: u8 = 0x01;
    /// I/O device attached/detached event from the hub.
    pub const HUB_ATTACHED_IO: u8 = 0x04;
    /// Outgoing command targeting a port.
    pub const PORT_OUTPUT: u8 = 0x81;
    /// Sensor value notification (e.g. speedometer reading).
    pub const PORT_VALUE: u8 = 0x45;
    /// Hub-side feedback for a previously sent `PORT_OUTPUT` command.
    pub const PORT_OUTPUT_FEEDBACK: u8 = 0x82;
}

/// Port output command flags (byte 4 of output frames).
mod output {
    /// Execute immediately + enable command feedback.
    pub const STARTUP_AND_COMPLETION: u8 = 0x11;
    /// `WriteDirectModeData` sub-command (byte 5).
    pub const WRITE_DIRECT_MODE_DATA: u8 = 0x51;
}

/// Multi-port sub-command bytes (byte 7 of frames addressed to `port::MULTI`).
mod subcmd {
    /// Set LED colour.
    pub const LED: u8 = 0x04;
    /// Trigger horn sound.
    pub const HORN: u8 = 0x07;
}

/// Bitmask values in `PORT_OUTPUT_FEEDBACK` frames (byte 4).
mod feedback {
    /// Command was executed successfully.
    pub const COMPLETED: u8 = 0x02;
    /// Command was rejected by the hub.
    pub const DISCARDED: u8 = 0x04;
    /// Port is busy and cannot accept the command right now.
    pub const BUSY: u8 = 0x10;
    /// Output buffer drained to empty.
    pub const BUFFER_EMPTY: u8 = 0x01;
    /// Output buffer has remaining capacity.
    pub const BUFFER_LOW: u8 = 0x08;
}

// ── Submodules ────────────────────────────────────────────────────────────────

mod buffer;
mod commands;
mod messages;

// ── Public re-exports (unchanged API) ────────────────────────────────────────

pub use buffer::MessageBuffer;
pub use commands::{
    battery_request, horn_command, led_color_command, motor_command, sound_packet,
    speedometer_subscribe,
};
pub use messages::{CommandFeedback, DeviceType, IoEvent, ParsedMessage, parse_message};
