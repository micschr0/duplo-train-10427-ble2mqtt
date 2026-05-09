//! Shared types for inter-actor communication.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};

/// High-level train commands from MQTT.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, AsRefStr, Display,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
#[serde(rename_all = "lowercase")]
pub enum TrainCommand {
    Forward,
    Boost,
    Backward,
    Stop,
}

impl TrainCommand {
    /// Returns true if this command should not be subject to duplicate blocking.
    pub fn allows_repeat(self) -> bool {
        matches!(self, Self::Forward | Self::Boost)
    }
}

/// LED color on the DUPLO train hub.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, AsRefStr, Display,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
pub enum LedColor {
    Off,
    White,
    Green,
    Yellow,
    LightBlue,
    DarkBlue,
    Purple,
    PurplePink,
    LightPink,
    RedPink,
    Red,
}

impl From<LedColor> for u8 {
    fn from(color: LedColor) -> u8 {
        match color {
            LedColor::Off => 0x00,
            LedColor::White => 0x01,
            LedColor::Green => 0x07,
            LedColor::Yellow => 0x08,
            LedColor::LightBlue => 0x09,
            LedColor::DarkBlue => 0x0F,
            LedColor::Purple => 0x0A,
            LedColor::PurplePink => 0x0E,
            LedColor::LightPink => 0x0B,
            LedColor::RedPink => 0x0D,
            LedColor::Red => 0x0C,
        }
    }
}

/// Pre-recorded sound on the DUPLO train hub speaker (port 0x01, mode 0x01).
///
/// Only these five sound IDs exist in the train firmware.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, AsRefStr, Display,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(rename_all = "snake_case")]
pub enum DuploSound {
    Brake,
    StationDeparture,
    WaterRefill,
    Horn,
    Steam,
}

impl From<DuploSound> for u8 {
    fn from(sound: DuploSound) -> u8 {
        match sound {
            DuploSound::Brake => 3,
            DuploSound::StationDeparture => 5,
            DuploSound::WaterRefill => 7,
            DuploSound::Horn => 9,
            DuploSound::Steam => 10,
        }
    }
}

/// Envelope that carries any command sent to the BLE actor.
///
/// Wraps the existing `TrainCommand` (motor/horn) plus auxiliary actions
/// (LED colour, sound) so they can share one channel without bloating the
/// motor-specific enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Train(TrainCommand),
    Led(LedColor),
    Sound(DuploSound),
}

/// Connection state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionState {
    #[default]
    Standby,
    Connecting,
    Connected,
}

/// Train state published to MQTT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainState {
    pub status: ConnectionState,
    pub attempts: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motor: Option<i8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led: Option<LedColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sound: Option<DuploSound>,
    pub ts: u64,
}

impl Default for TrainState {
    fn default() -> Self {
        Self::standby()
    }
}

impl TrainState {
    /// Create a standby state.
    pub fn standby() -> Self {
        Self {
            status: ConnectionState::Standby,
            attempts: 0,
            battery: None,
            motor: None,
            speed: None,
            led: None,
            last_sound: None,
            ts: current_timestamp(),
        }
    }

    /// Create a connected state.
    pub fn connected() -> Self {
        Self {
            status: ConnectionState::Connected,
            attempts: 0,
            battery: None,
            motor: Some(0),
            speed: Some(0),
            led: None,
            last_sound: None,
            ts: current_timestamp(),
        }
    }

    /// Update timestamp to now.
    pub fn touch(&mut self) {
        self.ts = current_timestamp();
    }

    /// Replace self with `new`, preserving fields that should survive
    /// standby/connect transitions (battery, LED, last sound).
    pub fn apply(&mut self, new: TrainState) {
        let battery = self.battery;
        let led = self.led;
        let last_sound = self.last_sound;
        *self = new;
        self.battery = battery;
        self.led = led;
        self.last_sound = last_sound;
    }
}

/// Get current Unix timestamp in seconds.
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Status updates sent from BLE actor to MQTT actor.
#[derive(Debug, Clone)]
pub enum StatusUpdate {
    /// Full state update.
    State(TrainState),
    /// Battery level update (0-100 percentage).
    Battery(u8),
    /// Motor speed confirmation (commanded value).
    Motor(i8),
    /// Speedometer reading (measured value).
    Speed(i16),
    /// LED color confirmation (last set value).
    Led(LedColor),
    /// Sound action confirmation (last triggered value).
    Sound(DuploSound),
    /// Connection state changed.
    ConnectionState(ConnectionState),
    /// Connection attempts changed (0-3, for HA feedback).
    Attempts(u8),
    /// Error occurred.
    Error(String),
}

/// Command execution result sent from BLE actor to MQTT actor.
///
/// Only motor/horn commands surface here — LED and sound state changes
/// are observable via `TrainState.led` / `TrainState.last_sound`.
#[derive(Debug, Clone)]
pub struct CommandExecuted {
    pub cmd: TrainCommand,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod train_command {
        use super::*;

        #[test]
        fn parse_valid_commands() {
            assert_eq!(
                "forward".parse::<TrainCommand>().unwrap(),
                TrainCommand::Forward
            );
            assert_eq!(
                "boost".parse::<TrainCommand>().unwrap(),
                TrainCommand::Boost
            );
            assert_eq!(
                "backward".parse::<TrainCommand>().unwrap(),
                TrainCommand::Backward
            );
            assert_eq!("stop".parse::<TrainCommand>().unwrap(), TrainCommand::Stop);
        }

        #[test]
        fn parse_case_insensitive() {
            assert_eq!(
                "FORWARD".parse::<TrainCommand>().unwrap(),
                TrainCommand::Forward
            );
            assert_eq!(
                "Forward".parse::<TrainCommand>().unwrap(),
                TrainCommand::Forward
            );
            assert_eq!(
                "fOrWaRd".parse::<TrainCommand>().unwrap(),
                TrainCommand::Forward
            );
        }

        #[test]
        fn parse_invalid_returns_error() {
            assert!("invalid".parse::<TrainCommand>().is_err());
            assert!("".parse::<TrainCommand>().is_err());
            assert!("50".parse::<TrainCommand>().is_err());
        }

        #[test]
        fn display_lowercase() {
            assert_eq!(TrainCommand::Forward.to_string(), "forward");
            assert_eq!(TrainCommand::Boost.to_string(), "boost");
            assert_eq!(TrainCommand::Backward.to_string(), "backward");
            assert_eq!(TrainCommand::Stop.to_string(), "stop");
        }

        #[test]
        fn allows_repeat_for_forward_and_boost() {
            assert!(TrainCommand::Forward.allows_repeat());
            assert!(TrainCommand::Boost.allows_repeat());
            assert!(!TrainCommand::Backward.allows_repeat());
            assert!(!TrainCommand::Stop.allows_repeat());
        }

        #[test]
        fn json_serialization() {
            assert_eq!(
                serde_json::to_string(&TrainCommand::Forward).unwrap(),
                "\"forward\""
            );
            assert_eq!(
                serde_json::to_string(&TrainCommand::Stop).unwrap(),
                "\"stop\""
            );
        }
    }

    mod connection_state {
        use super::*;

        #[test]
        fn default_is_standby() {
            assert_eq!(ConnectionState::default(), ConnectionState::Standby);
        }

        #[test]
        fn json_serialization() {
            assert_eq!(
                serde_json::to_string(&ConnectionState::Standby).unwrap(),
                "\"standby\""
            );
            assert_eq!(
                serde_json::to_string(&ConnectionState::Connecting).unwrap(),
                "\"connecting\""
            );
            assert_eq!(
                serde_json::to_string(&ConnectionState::Connected).unwrap(),
                "\"connected\""
            );
        }
    }

    mod train_state {
        use super::*;

        #[test]
        fn standby_state() {
            let state = TrainState::standby();
            assert_eq!(state.status, ConnectionState::Standby);
            assert_eq!(state.attempts, 0);
            assert!(state.battery.is_none());
            assert!(state.motor.is_none());
            assert!(state.ts > 0);
        }

        #[test]
        fn connected_state() {
            let state = TrainState::connected();
            assert_eq!(state.status, ConnectionState::Connected);
            assert_eq!(state.attempts, 0);
            assert_eq!(state.motor, Some(0));
        }

        #[test]
        fn json_serialization_skips_none() {
            let state = TrainState::standby();
            let json = serde_json::to_string(&state).unwrap();
            assert!(!json.contains("battery"));
            assert!(!json.contains("motor"));
            assert!(json.contains("status"));
            assert!(json.contains("attempts"));
            assert!(json.contains("ts"));
        }

        #[test]
        fn json_includes_present_values() {
            let mut state = TrainState::connected();
            state.battery = Some(75);
            state.motor = Some(50);
            state.speed = Some(42);
            let json = serde_json::to_string(&state).unwrap();
            assert!(json.contains("\"battery\":75"));
            assert!(json.contains("\"motor\":50"));
            assert!(json.contains("\"speed\":42"));
        }

        #[test]
        fn json_roundtrip() {
            let state = TrainState {
                status: ConnectionState::Connected,
                attempts: 0,
                battery: Some(80),
                motor: Some(-50),
                speed: Some(-30),
                led: Some(LedColor::Red),
                last_sound: Some(DuploSound::Horn),
                ts: 1234567890,
            };
            let json = serde_json::to_string(&state).unwrap();
            let parsed: TrainState = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.status, state.status);
            assert_eq!(parsed.attempts, state.attempts);
            assert_eq!(parsed.battery, state.battery);
            assert_eq!(parsed.motor, state.motor);
            assert_eq!(parsed.speed, state.speed);
            assert_eq!(parsed.led, state.led);
            assert_eq!(parsed.last_sound, state.last_sound);
            assert_eq!(parsed.ts, state.ts);
        }
    }

    mod led_color {
        use super::*;

        #[test]
        fn parse_case_insensitive() {
            assert_eq!("red".parse::<LedColor>().unwrap(), LedColor::Red);
            assert_eq!("RED".parse::<LedColor>().unwrap(), LedColor::Red);
            assert_eq!(
                "light_blue".parse::<LedColor>().unwrap(),
                LedColor::LightBlue
            );
        }

        #[test]
        fn parse_invalid_returns_error() {
            assert!("invalid".parse::<LedColor>().is_err());
            assert!("".parse::<LedColor>().is_err());
        }

        #[test]
        fn into_u8_matches_protocol() {
            assert_eq!(u8::from(LedColor::Off), 0x00);
            assert_eq!(u8::from(LedColor::White), 0x01);
            assert_eq!(u8::from(LedColor::Green), 0x07);
            assert_eq!(u8::from(LedColor::Yellow), 0x08);
            assert_eq!(u8::from(LedColor::LightBlue), 0x09);
            assert_eq!(u8::from(LedColor::DarkBlue), 0x0F);
            assert_eq!(u8::from(LedColor::Purple), 0x0A);
            assert_eq!(u8::from(LedColor::PurplePink), 0x0E);
            assert_eq!(u8::from(LedColor::LightPink), 0x0B);
            assert_eq!(u8::from(LedColor::RedPink), 0x0D);
            assert_eq!(u8::from(LedColor::Red), 0x0C);
        }

        #[test]
        fn json_serialization_snake_case() {
            assert_eq!(
                serde_json::to_string(&LedColor::LightBlue).unwrap(),
                "\"light_blue\""
            );
        }
    }

    mod duplo_sound {
        use super::*;

        #[test]
        fn parse_case_insensitive() {
            assert_eq!("brake".parse::<DuploSound>().unwrap(), DuploSound::Brake);
            assert_eq!("BRAKE".parse::<DuploSound>().unwrap(), DuploSound::Brake);
            assert_eq!(
                "station_departure".parse::<DuploSound>().unwrap(),
                DuploSound::StationDeparture
            );
        }

        #[test]
        fn parse_invalid_returns_error() {
            assert!("invalid".parse::<DuploSound>().is_err());
            assert!("".parse::<DuploSound>().is_err());
            assert!("silence".parse::<DuploSound>().is_err());
        }

        #[test]
        fn into_u8_matches_protocol() {
            assert_eq!(u8::from(DuploSound::Brake), 3);
            assert_eq!(u8::from(DuploSound::StationDeparture), 5);
            assert_eq!(u8::from(DuploSound::WaterRefill), 7);
            assert_eq!(u8::from(DuploSound::Horn), 9);
            assert_eq!(u8::from(DuploSound::Steam), 10);
        }

        #[test]
        fn json_serialization_snake_case() {
            assert_eq!(
                serde_json::to_string(&DuploSound::StationDeparture).unwrap(),
                "\"station_departure\""
            );
            assert_eq!(
                serde_json::to_string(&DuploSound::WaterRefill).unwrap(),
                "\"water_refill\""
            );
        }
    }
}
