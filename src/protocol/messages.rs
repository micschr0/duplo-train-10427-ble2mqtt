//! Incoming BLE notification types and parser for LWP messages.

use super::{feedback, msg, port};

/// I/O event type for Hub Attached I/O messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoEvent {
    Detached,
    Attached,
    AttachedVirtual,
}

/// Known device types from LEGO Wireless Protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    DuploTrainMotor,
    DuploTrainSpeaker,
    DuploTrainColorSensor,
    DuploTrainSpeedometer,
    RgbLight,
    Voltage,
    Unknown(u16),
}

impl DeviceType {
    pub fn from_id(id: u16) -> Self {
        match id {
            0x0029 => DeviceType::DuploTrainColorSensor,
            0x002A => DeviceType::DuploTrainMotor,
            0x002B => DeviceType::DuploTrainSpeaker,
            0x002C => DeviceType::DuploTrainSpeedometer,
            0x0017 => DeviceType::RgbLight,
            0x0014 => DeviceType::Voltage,
            0x005A => DeviceType::DuploTrainMotor,
            0x005B => DeviceType::DuploTrainSpeedometer,
            other => DeviceType::Unknown(other),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            DeviceType::DuploTrainMotor => "DUPLO Train Motor",
            DeviceType::DuploTrainSpeaker => "DUPLO Train Speaker",
            DeviceType::DuploTrainColorSensor => "DUPLO Train Color Sensor",
            DeviceType::DuploTrainSpeedometer => "DUPLO Train Speedometer",
            DeviceType::RgbLight => "RGB Light",
            DeviceType::Voltage => "Voltage Sensor",
            DeviceType::Unknown(_) => "Unknown Device",
        }
    }
}

/// Result of a command execution as reported by the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFeedback {
    Completed,
    Discarded,
    Busy,
    BufferUpdate { empty: bool },
}

/// Parsed message from a BLE notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedMessage {
    Battery(u8),
    HubAttachedIo {
        port_id: u8,
        event: IoEvent,
        device_type: Option<DeviceType>,
    },
    Speedometer(i16),
    Feedback {
        port_id: u8,
        feedback: CommandFeedback,
    },
    Unknown {
        msg_type: u8,
        data: Vec<u8>,
    },
}

/// Parse a complete LWP message from a BLE notification payload.
pub fn parse_message(data: &[u8]) -> Option<ParsedMessage> {
    if data.len() < 3 {
        return None;
    }
    let msg_type = data[2];
    match msg_type {
        msg::PROPERTY => parse_property_message(data),
        msg::HUB_ATTACHED_IO => parse_hub_attached_io(data),
        msg::PORT_VALUE => parse_port_value(data),
        msg::PORT_OUTPUT_FEEDBACK => parse_feedback_message(data),
        _ => Some(ParsedMessage::Unknown {
            msg_type,
            data: data.to_vec(),
        }),
    }
}

fn parse_property_message(data: &[u8]) -> Option<ParsedMessage> {
    if data.len() >= 6 && data[3] == 0x06 {
        Some(ParsedMessage::Battery(data[5]))
    } else {
        Some(ParsedMessage::Unknown {
            msg_type: msg::PROPERTY,
            data: data.to_vec(),
        })
    }
}

fn parse_hub_attached_io(data: &[u8]) -> Option<ParsedMessage> {
    if data.len() < 5 {
        return None;
    }
    let port_id = data[3];
    let event_byte = data[4];

    let (event, device_type) = match event_byte {
        0 => (IoEvent::Detached, None),
        1 => {
            if data.len() >= 7 {
                let type_id = u16::from_le_bytes([data[5], data[6]]);
                (IoEvent::Attached, Some(DeviceType::from_id(type_id)))
            } else {
                (IoEvent::Attached, None)
            }
        }
        2 => {
            if data.len() >= 7 {
                let type_id = u16::from_le_bytes([data[5], data[6]]);
                (IoEvent::AttachedVirtual, Some(DeviceType::from_id(type_id)))
            } else {
                (IoEvent::AttachedVirtual, None)
            }
        }
        _ => {
            return Some(ParsedMessage::Unknown {
                msg_type: msg::HUB_ATTACHED_IO,
                data: data.to_vec(),
            });
        }
    };

    Some(ParsedMessage::HubAttachedIo {
        port_id,
        event,
        device_type,
    })
}

fn parse_port_value(data: &[u8]) -> Option<ParsedMessage> {
    if data.len() < 5 {
        return None;
    }
    let port_id = data[3];
    match port_id {
        port::SPEEDOMETER | port::SPEEDOMETER_ALT => {
            if data.len() >= 6 {
                Some(ParsedMessage::Speedometer(i16::from_le_bytes([
                    data[4], data[5],
                ])))
            } else {
                Some(ParsedMessage::Speedometer(data[4] as i8 as i16))
            }
        }
        _ => Some(ParsedMessage::Unknown {
            msg_type: msg::PORT_VALUE,
            data: data.to_vec(),
        }),
    }
}

fn parse_feedback_message(data: &[u8]) -> Option<ParsedMessage> {
    if data.len() < 5 {
        return None;
    }
    let port_id = data[3];
    let b = data[4];
    let fb = if b & feedback::COMPLETED != 0 {
        CommandFeedback::Completed
    } else if b & feedback::DISCARDED != 0 {
        CommandFeedback::Discarded
    } else if b & feedback::BUSY != 0 {
        CommandFeedback::Busy
    } else {
        let empty = b & feedback::BUFFER_EMPTY != 0;
        CommandFeedback::BufferUpdate { empty }
    };
    Some(ParsedMessage::Feedback {
        port_id,
        feedback: fb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod message_parsing {
        use super::*;

        #[test]
        fn parse_battery_message() {
            let data = vec![0x06, 0x00, 0x01, 0x06, 0x05, 75];
            assert_eq!(parse_message(&data), Some(ParsedMessage::Battery(75)));
        }

        #[test]
        fn parse_feedback_completed() {
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x02];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::Completed
                })
            );
        }

        #[test]
        fn parse_feedback_discarded() {
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x04];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::Discarded
                })
            );
        }

        #[test]
        fn parse_feedback_busy() {
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x10];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::Busy
                })
            );
        }

        #[test]
        fn parse_feedback_buffer_empty() {
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x01];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::BufferUpdate { empty: true }
                })
            );
        }

        #[test]
        fn parse_feedback_buffer_not_empty() {
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x08];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::BufferUpdate { empty: false }
                })
            );
        }

        #[test]
        fn parse_feedback_zero_byte_not_empty() {
            // 0x00 means no flags set — BUFFER_EMPTY bit is not set, so empty must be false.
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x00];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::BufferUpdate { empty: false }
                })
            );
        }

        #[test]
        fn parse_unknown_message_type() {
            let data = vec![0x05, 0x00, 0xFF, 0x01, 0x02];
            assert!(matches!(
                parse_message(&data),
                Some(ParsedMessage::Unknown { msg_type: 0xFF, .. })
            ));
        }

        #[test]
        fn parse_too_short_message() {
            assert!(parse_message(&[0x02, 0x00]).is_none());
        }

        #[test]
        fn parse_empty_message() {
            assert!(parse_message(&[]).is_none());
        }
    }

    mod feedback_priority {
        use super::*;

        #[test]
        fn completed_takes_priority_over_buffer() {
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x03];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::Completed
                })
            );
        }

        #[test]
        fn discarded_takes_priority_over_buffer() {
            let data = vec![0x05, 0x00, 0x82, 0x32, 0x05];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Feedback {
                    port_id: 0x32,
                    feedback: CommandFeedback::Discarded
                })
            );
        }
    }

    mod device_type {
        use super::*;

        #[test]
        fn from_id_duplo_train_motor() {
            assert_eq!(DeviceType::from_id(0x002A), DeviceType::DuploTrainMotor);
        }

        #[test]
        fn from_id_duplo_train_motor_alternate() {
            assert_eq!(DeviceType::from_id(0x005A), DeviceType::DuploTrainMotor);
        }

        #[test]
        fn from_id_duplo_train_speaker() {
            assert_eq!(DeviceType::from_id(0x002B), DeviceType::DuploTrainSpeaker);
        }

        #[test]
        fn from_id_duplo_train_color_sensor() {
            assert_eq!(
                DeviceType::from_id(0x0029),
                DeviceType::DuploTrainColorSensor
            );
        }

        #[test]
        fn from_id_duplo_train_speedometer() {
            assert_eq!(
                DeviceType::from_id(0x002C),
                DeviceType::DuploTrainSpeedometer
            );
        }

        #[test]
        fn from_id_duplo_train_speedometer_alternate() {
            assert_eq!(
                DeviceType::from_id(0x005B),
                DeviceType::DuploTrainSpeedometer
            );
        }

        #[test]
        fn from_id_rgb_light() {
            assert_eq!(DeviceType::from_id(0x0017), DeviceType::RgbLight);
        }

        #[test]
        fn from_id_voltage() {
            assert_eq!(DeviceType::from_id(0x0014), DeviceType::Voltage);
        }

        #[test]
        fn from_id_unknown() {
            assert_eq!(DeviceType::from_id(0x9999), DeviceType::Unknown(0x9999));
        }

        #[test]
        fn name_returns_correct_strings() {
            assert_eq!(DeviceType::DuploTrainMotor.name(), "DUPLO Train Motor");
            assert_eq!(DeviceType::DuploTrainSpeaker.name(), "DUPLO Train Speaker");
            assert_eq!(
                DeviceType::DuploTrainColorSensor.name(),
                "DUPLO Train Color Sensor"
            );
            assert_eq!(
                DeviceType::DuploTrainSpeedometer.name(),
                "DUPLO Train Speedometer"
            );
            assert_eq!(DeviceType::RgbLight.name(), "RGB Light");
            assert_eq!(DeviceType::Voltage.name(), "Voltage Sensor");
            assert_eq!(DeviceType::Unknown(0x1234).name(), "Unknown Device");
        }
    }

    mod io_event {
        use super::*;

        #[test]
        fn variants_exist() {
            let _detached = IoEvent::Detached;
            let _attached = IoEvent::Attached;
            let _virtual = IoEvent::AttachedVirtual;
        }

        #[test]
        fn equality() {
            assert_eq!(IoEvent::Attached, IoEvent::Attached);
            assert_ne!(IoEvent::Attached, IoEvent::Detached);
        }
    }

    mod hub_attached_io {
        use super::*;

        #[test]
        fn parse_device_attached() {
            let data = vec![
                0x0F, 0x00, 0x04, 0x34, 0x01, 0x5A, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
                0x00,
            ];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::HubAttachedIo {
                    port_id: 0x34,
                    event: IoEvent::Attached,
                    device_type: Some(DeviceType::DuploTrainMotor),
                })
            );
        }

        #[test]
        fn parse_device_detached() {
            let data = vec![0x05, 0x00, 0x04, 0x34, 0x00];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::HubAttachedIo {
                    port_id: 0x34,
                    event: IoEvent::Detached,
                    device_type: None,
                })
            );
        }

        #[test]
        fn parse_virtual_device_attached() {
            let data = vec![0x07, 0x00, 0x04, 0x10, 0x02, 0x2C, 0x00];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::HubAttachedIo {
                    port_id: 0x10,
                    event: IoEvent::AttachedVirtual,
                    device_type: Some(DeviceType::DuploTrainSpeedometer),
                })
            );
        }

        #[test]
        fn parse_speedometer_attached() {
            let data = vec![
                0x0F, 0x00, 0x04, 0x36, 0x01, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00,
            ];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::HubAttachedIo {
                    port_id: 0x36,
                    event: IoEvent::Attached,
                    device_type: Some(DeviceType::DuploTrainSpeedometer),
                })
            );
        }

        #[test]
        fn parse_too_short_returns_none() {
            let data = vec![0x04, 0x00, 0x04, 0x34];
            assert!(parse_message(&data).is_none());
        }

        #[test]
        fn parse_unknown_event_returns_unknown() {
            let data = vec![0x05, 0x00, 0x04, 0x34, 0x99];
            assert!(matches!(
                parse_message(&data),
                Some(ParsedMessage::Unknown { msg_type: 0x04, .. })
            ));
        }
    }

    mod speedometer {
        use super::*;

        #[test]
        fn parse_speedometer_zero() {
            let data = vec![0x06, 0x00, 0x45, 0x36, 0x00, 0x00];
            assert_eq!(parse_message(&data), Some(ParsedMessage::Speedometer(0)));
        }

        #[test]
        fn parse_speedometer_positive() {
            let data = vec![0x06, 0x00, 0x45, 0x36, 0x2A, 0x00];
            assert_eq!(parse_message(&data), Some(ParsedMessage::Speedometer(42)));
        }

        #[test]
        fn parse_speedometer_negative() {
            let data = vec![0x06, 0x00, 0x45, 0x36, 0xD6, 0xFF];
            assert_eq!(parse_message(&data), Some(ParsedMessage::Speedometer(-42)));
        }

        #[test]
        fn parse_speedometer_max_positive() {
            let data = vec![0x06, 0x00, 0x45, 0x36, 0xFF, 0x7F];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Speedometer(32767))
            );
        }

        #[test]
        fn parse_speedometer_max_negative() {
            let data = vec![0x06, 0x00, 0x45, 0x36, 0x00, 0x80];
            assert_eq!(
                parse_message(&data),
                Some(ParsedMessage::Speedometer(-32768))
            );
        }

        #[test]
        fn parse_speedometer_alternate_port() {
            let data = vec![0x06, 0x00, 0x45, 0x33, 0x2A, 0x00];
            assert_eq!(parse_message(&data), Some(ParsedMessage::Speedometer(42)));
        }

        #[test]
        fn parse_port_value_unknown_port() {
            let data = vec![0x06, 0x00, 0x45, 0x99, 0x2A, 0x00];
            assert!(matches!(
                parse_message(&data),
                Some(ParsedMessage::Unknown { msg_type: 0x45, .. })
            ));
        }
    }
}
