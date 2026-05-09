# DUPLO Train Controller

[![CI](https://github.com/micschr0/duplo-train-10427-ble2mqtt/actions/workflows/ci.yml/badge.svg)](https://github.com/micschr0/duplo-train-10427-ble2mqtt/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

BLE-to-MQTT bridge for LEGO DUPLO 10427 train. Enables Home Assistant integration via MQTT.

> **Disclaimer:** This is my first venture into Rust. It was also an experiment to see how a coding agent like Claude Code works in practice.

## About

This service runs on a Raspberry Pi 4 and bridges the LEGO DUPLO train's Bluetooth LE interface to MQTT. Combined with Home Assistant, this allows controlling the train using Zigbee buttons, switches, or any other Home Assistant events.

**Use case:** My kid can press a Zigbee remote with 4 coloured buttons to start/stop the train. Home Assistant bridges the button events to MQTT commands and provides feedback:
- **Visual feedback:** Lamps briefly flash in different colors to confirm commands (green = forward, red = stop, etc.)
- **Mode indication:** A lamp changes color to show current train mode
- **TTS announcements:** Speakers give voice hints when the train needs to be turned on/off (e.g., "Please wake up the train" after connection timeout)

## Compatibility

Tested with:
- **Train:** LEGO DUPLO 10427 (Steam Train)
- **Host:** Raspberry Pi 4 (aarch64), macOS (Apple Silicon)
- **Bluetooth:** Built-in BLE on both platforms

May also work with other new generation DUPLO trains.

## Requirements

- Rust 1.85+
- Bluetooth LE adapter
- MQTT broker (e.g., Mosquitto)
- Home Assistant (for bridging events to commands)

## Installation

```bash
cargo build --release
```

Cross-compilation for Raspberry Pi 4:
```bash
./scripts/build-rpi4.sh
```

## Configuration

Environment variables (or `.env` file):

| Variable | Default | Description |
|----------|---------|-------------|
| `MQTT_HOST` | `localhost` | MQTT broker host |
| `MQTT_PORT` | `1883` | MQTT broker port |
| `MQTT_USERNAME` | - | MQTT username (optional) |
| `MQTT_PASSWORD` | - | MQTT password (optional) |
| `MQTT_CLIENT_ID` | `duplo-train-gateway` | MQTT client identifier |
| `MQTT_BASE_TOPIC` | `duplo/train` | Base topic for all MQTT messages |
| `MOTOR_FORWARD` | `50` | Forward speed (1–100) |
| `MOTOR_BOOST` | `75` | Boost speed (1–100) |
| `MOTOR_BOOST_DURATION` | - | Auto-revert boost after N seconds (0 = unlimited) |
| `MOTOR_BACKWARD` | `-50` | Backward speed (-100 to -1) |
| `BACKWARD_DELAY` | `1200` | Delay before backward (ms) |

## Usage

```bash
# With .env file
./duplo-train-controller

# With environment variables
MQTT_HOST=192.168.1.100 RUST_LOG=info ./duplo-train-controller
```

BLE permissions on Linux:
```bash
sudo setcap cap_net_raw,cap_net_admin+eip ./duplo-train-controller
```

## MQTT Interface

### Subscribe

| Topic | Payload | Description |
|-------|---------|-------------|
| `duplo/train/cmd` | `forward` | Drive forward |
| | `boost` | Drive forward fast |
| | `backward` | Horn + reverse |
| | `stop` | Stop motor |
| `duplo/train/led/set` | `off`, `white`, `green`, `yellow`, `light_blue`, `dark_blue`, `purple`, `purple_pink`, `light_pink`, `red_pink`, `red` | Set hub LED color |
| `duplo/train/sound/set` | `horn`, `brake`, `steam`, `station_departure`, `water_refill` | Play train sound |

### Publish

**Availability** (`duplo/train/availability`, retained):
| Payload | Description |
|---------|-------------|
| `online` | Service connected to MQTT broker |
| `offline` | Service disconnected (via LWT) |

**State** (`duplo/train/state`, retained):
```json
{
  "status": "standby|connecting|connected",
  "attempts": 0,
  "battery": 85,
  "motor": 50,
  "speed": 42,
  "ts": 1234567890
}
```

- `status`: `standby` (waiting for command), `connecting` (BLE scan), `connected` (BLE active)
- `motor`: Commanded speed (-100 to 100)
- `speed`: Measured speed from speedometer (only when connected)

**Executed** (`duplo/train/executed`, not retained):
```json
{"cmd": "forward"}
```

## Connection Behavior

- BLE connection on first command (lazy connect)
- Auto-disconnect after 5 min idle (saves train battery)
- 3 connection attempts, then 25s cooldown
- 120s timeout while connecting

## Optional: Home Assistant Integration

See [HOMEASSISTANT.md](./HOMEASSISTANT.md) for:
- MQTT Sensor configuration
- Automation examples
- Dashboard cards ([example dashboard](./examples/dashboard.yml))

Basic automation to control the train with a Zigbee button:

```yaml
automation:
  - alias: "DUPLO Train - Green Button Forward"
    trigger:
      - platform: device
        device_id: <your_zigbee_button>
        type: remote_button_short_press
        subtype: button_1
    action:
      - service: mqtt.publish
        data:
          topic: duplo/train/cmd
          payload: forward

  - alias: "DUPLO Train - Red Button Stop"
    trigger:
      - platform: device
        device_id: <your_zigbee_button>
        type: remote_button_short_press
        subtype: button_2
    action:
      - service: mqtt.publish
        data:
          topic: duplo/train/cmd
          payload: stop
```

You can also create sensors from the train state:

```yaml
mqtt:
  sensor:
    - name: "DUPLO Train Battery"
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.battery }}"
      unit_of_measurement: "%"
      device_class: battery

    - name: "DUPLO Train Status"
      state_topic: "duplo/train/state"
      value_template: "{{ value_json.status }}"
```

## Architecture

```
                                                                      ┌────────────┐
                                                                      │ Zigbee     │
                                                               Zigbee │ Remote     │
                                                                      └─────┬──────┘
                                                                            │
                                                                            ▼
┌─────────────┐      BLE       ┌─────────────────────┐      MQTT      ┌────────────────┐
│ DUPLO Train │ <────────────> │ duplo-train-ctrl    │ <────────────> │ Home Assistant │
│   (10427)   │                │ (Raspberry Pi)      │                │                │
└─────────────┘                └─────────────────────┘                └────────┬───────┘
                                                                            │
                                                        ┌───────────────────┼───────────────────┐
                                                        │                   │                   │
                                                        ▼                   ▼                   ▼
                                                 ┌────────────┐      ┌────────────┐      ┌────────────┐
                                                 │ Zigbee     │      │ Smart      │      │ Media      │
                                                 │ Lights     │      │ Speaker    │      │ Player     │
                                                 │ (feedback) │      │ (TTS)      │      │ (sounds)   │
                                                 └────────────┘      └────────────┘      └────────────┘
```

## Troubleshooting

**Train not found:**
- Make sure the train is awake (press button on train)
- Check if Bluetooth is enabled: `bluetoothctl show`
- Try manual scan: `bluetoothctl scan on`

**Connection fails on Linux:**
- Set BLE capabilities: `sudo setcap cap_net_raw,cap_net_admin+eip ./duplo-train-controller`
- Try if it will run as root: `sudo ./duplo-train-controller`
- Clear BlueZ cache: `sudo rm -rf /var/lib/bluetooth/*/cache/*`

**Commands sent but train doesn't move:**
- Check if notifications are received (look for "Device attached" in logs)
- Restart Bluetooth: `sudo systemctl restart bluetooth`

**Integration tests fail:**
- Integration tests use `testcontainers` and require Docker to be running
- Run `docker info` to check if Docker is available
- Unit tests work without Docker: `cargo test --lib`

## TODO

- [ ] Color sensor readout for custom automations (e.g., trigger actions based on track color tiles)

## License

MIT
