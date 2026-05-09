//! Outgoing BLE command packet builders for the DUPLO train hub.

use super::{msg, output, port, subcmd};

/// Motor command. Speed: -100 to 100 (negative = backward, 0 = stop).
pub fn motor_command(speed: i8) -> Vec<u8> {
    vec![
        0x08,
        0x00,
        msg::PORT_OUTPUT,
        port::MOTOR,
        output::STARTUP_AND_COMPLETION,
        output::WRITE_DIRECT_MODE_DATA,
        0x00, // mode 0 (power)
        speed as u8,
    ]
}

fn multi_port_command(subcmd_byte: u8, value: u8) -> Vec<u8> {
    vec![
        0x0b,
        0x00,
        msg::PORT_OUTPUT,
        port::MULTI,
        output::STARTUP_AND_COMPLETION,
        output::WRITE_DIRECT_MODE_DATA,
        0x01, // mode 1 (multi-port action)
        subcmd_byte,
        0x01,
        value,
        0x00,
    ]
}

pub fn horn_command() -> Vec<u8> {
    multi_port_command(subcmd::HORN, 0x00)
}

pub fn led_color_command(color: u8) -> Vec<u8> {
    multi_port_command(subcmd::LED, color)
}

/// Speaker sound packet. Targets port 0x01, mode 0x01 — the real speaker
/// channel. Only firmware-supported IDs produce audio (see `DuploSound`).
pub fn sound_packet(id: u8) -> Vec<u8> {
    vec![
        0x08,
        0x00,
        msg::PORT_OUTPUT,
        port::SPEAKER,
        output::STARTUP_AND_COMPLETION,
        output::WRITE_DIRECT_MODE_DATA,
        port::SPEAKER_MODE,
        id,
    ]
}

pub fn battery_request() -> Vec<u8> {
    vec![0x05, 0x00, msg::PROPERTY, 0x06, 0x05]
}

/// Subscribe to speedometer updates on `port_id`.
/// `delta_interval` is the minimum change before a new notification is sent.
#[rustfmt::skip]
pub fn speedometer_subscribe(port_id: u8, delta_interval: u32) -> Vec<u8> {
    let d = delta_interval.to_le_bytes();
    vec![
        0x0A,    // Length
        0x00,    // Hub ID
        0x41,    // Port Input Format Setup (Single)
        port_id,
        0x00,    // Mode 0 (speed)
        d[0], d[1], d[2], d[3],
        0x01,    // Notification enabled
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motor_command_forward() {
        assert_eq!(
            motor_command(50),
            vec![0x08, 0x00, 0x81, 0x32, 0x11, 0x51, 0x00, 0x32]
        );
    }

    #[test]
    fn motor_command_backward() {
        assert_eq!(
            motor_command(-50),
            vec![0x08, 0x00, 0x81, 0x32, 0x11, 0x51, 0x00, 0xCE]
        );
    }

    #[test]
    fn motor_command_stop() {
        assert_eq!(
            motor_command(0),
            vec![0x08, 0x00, 0x81, 0x32, 0x11, 0x51, 0x00, 0x00]
        );
    }

    #[test]
    fn horn_command_packet() {
        assert_eq!(
            horn_command(),
            vec![
                0x0b, 0x00, 0x81, 0x34, 0x11, 0x51, 0x01, 0x07, 0x01, 0x00, 0x00
            ]
        );
    }

    #[test]
    fn led_color_command_off() {
        assert_eq!(
            led_color_command(0x00),
            vec![
                0x0b, 0x00, 0x81, 0x34, 0x11, 0x51, 0x01, 0x04, 0x01, 0x00, 0x00
            ]
        );
    }

    #[test]
    fn led_color_command_red() {
        assert_eq!(
            led_color_command(0x0C),
            vec![
                0x0b, 0x00, 0x81, 0x34, 0x11, 0x51, 0x01, 0x04, 0x01, 0x0C, 0x00
            ]
        );
    }

    #[test]
    fn sound_packet_horn() {
        assert_eq!(
            sound_packet(9),
            vec![0x08, 0x00, 0x81, 0x01, 0x11, 0x51, 0x01, 0x09]
        );
    }

    #[test]
    fn sound_packet_steam() {
        assert_eq!(
            sound_packet(10),
            vec![0x08, 0x00, 0x81, 0x01, 0x11, 0x51, 0x01, 0x0A]
        );
    }

    #[test]
    fn sound_packet_brake() {
        assert_eq!(
            sound_packet(3),
            vec![0x08, 0x00, 0x81, 0x01, 0x11, 0x51, 0x01, 0x03]
        );
    }

    #[test]
    fn battery_request_packet() {
        assert_eq!(battery_request(), vec![0x05, 0x00, 0x01, 0x06, 0x05]);
    }

    mod backward_sequence {
        use super::*;

        #[test]
        fn backward_sequence_stop_command() {
            assert_eq!(motor_command(0)[7], 0x00);
        }

        #[test]
        fn backward_sequence_horn_command() {
            let horn = horn_command();
            assert_eq!(horn[3], 0x34); // port::MULTI
            assert_eq!(horn[7], 0x07); // subcmd::HORN
        }

        #[test]
        fn backward_sequence_motor_command() {
            assert_eq!(motor_command(-50)[7], 0xCE);
        }

        #[test]
        fn backward_sequence_all_commands_valid() {
            let stop = motor_command(0);
            let horn = horn_command();
            let backward = motor_command(-50);
            assert_eq!(stop[2], 0x81);
            assert_eq!(horn[2], 0x81);
            assert_eq!(backward[2], 0x81);
            assert_eq!(stop[3], 0x32);
            assert_eq!(backward[3], 0x32);
            assert_eq!(horn[3], 0x34);
        }
    }

    mod motor_speed_boundaries {
        use super::*;

        #[test]
        fn motor_command_max_forward() {
            assert_eq!(motor_command(100)[7], 0x64);
        }

        #[test]
        fn motor_command_max_backward() {
            assert_eq!(motor_command(-100)[7], 0x9C);
        }

        #[test]
        fn motor_command_default_forward() {
            assert_eq!(motor_command(50)[7], 0x32);
        }

        #[test]
        fn motor_command_default_boost() {
            assert_eq!(motor_command(75)[7], 0x4B);
        }

        #[test]
        fn motor_command_default_backward() {
            assert_eq!(motor_command(-50)[7], 0xCE);
        }
    }

    mod speedometer_subscribe {
        use super::*;

        #[test]
        fn command_structure_default_port() {
            let cmd = speedometer_subscribe(0x36, 1);
            assert_eq!(cmd[0], 0x0A);
            assert_eq!(cmd[1], 0x00);
            assert_eq!(cmd[2], 0x41);
            assert_eq!(cmd[3], 0x36);
            assert_eq!(cmd[4], 0x00);
            assert_eq!(cmd[9], 0x01);
        }

        #[test]
        fn command_structure_alternate_port() {
            let cmd = speedometer_subscribe(0x33, 1);
            assert_eq!(cmd[3], 0x33);
        }

        #[test]
        fn delta_interval_encoding() {
            let cmd = speedometer_subscribe(0x36, 0);
            assert_eq!(&cmd[5..9], &[0x00, 0x00, 0x00, 0x00]);

            let cmd = speedometer_subscribe(0x36, 1);
            assert_eq!(&cmd[5..9], &[0x01, 0x00, 0x00, 0x00]);

            let cmd = speedometer_subscribe(0x36, 256);
            assert_eq!(&cmd[5..9], &[0x00, 0x01, 0x00, 0x00]);
        }
    }
}
