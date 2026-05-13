# DUPLO Train Controller

[![CI](https://github.com/micschr0/duplo-train-10427-ble2mqtt/actions/workflows/ci.yml/badge.svg)](https://github.com/micschr0/duplo-train-10427-ble2mqtt/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

BLE-to-MQTT bridge for LEGO DUPLO 10427 train. Enables Home Assistant integration via MQTT.

> This is my first venture into Rust. It was also an experiment to see how a coding agent like Claude Code works in practice.

## About

This service runs on a Raspberry Pi 4 and bridges the LEGO DUPLO train's Bluetooth LE interface to MQTT. Combined with Home Assistant, this lets you control the train with Zigbee buttons, switches, or any other Home Assistant events.

This project came about because none of the established LEGO BLE libraries work with current DUPLO train models (the 2025 generation) — see [Background](#background) for details.

## Use Case

My kid can press a Zigbee remote with 4 coloured buttons to start/stop the train. Home Assistant bridges the button events to MQTT commands and runs these reactions:
- **Visual feedback:** Lamps briefly flash in different colors to confirm commands (green = forward, red = stop, etc.)
- **Mode indication:** A lamp changes color to show current train mode
- **Light scenes:** Home Assistant runs light sequences — e.g., a coordinated colour sweep across the room when the train enters boost mode
- **Boost sound:** When boost kicks in, a smart speaker plays a sound effect
- **TTS announcements:** Speakers give voice hints when the train needs to be turned on/off (e.g., "Please wake up the train" after connection timeout)

With Home Assistant in the middle, anything it can react to or control plugs into this flow — buttons are just one trigger, lamps and speakers just two reactions.

## Compatibility

Tested with:
- **Train:** LEGO DUPLO 10427 (Interactive Adventure Train)
- **Host:** Raspberry Pi 4 (aarch64, 64-bit Raspberry Pi OS) and
  macOS (Apple Silicon) — both verified end-to-end against a real train.
- **Bluetooth:** Built-in BLE on both platforms.

Other recent DUPLO trains likely work as well. In particular, the
**LEGO DUPLO 10428 (Cargo Train)** uses the same LEGO Wireless Protocol 3
(LWP3) hub family based on community research, so it should be a drop-in. **I have
not verified this myself** — if you try it, please report back via an issue
with the result.

> **Older DUPLO trains (10874, 10875) need a different library** like
> [Legoino](https://github.com/corneliusmunz/legoino) — details in
> [FAQ → Which trains are supported?](./docs/FAQ.md#which-trains-are-supported).

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

For the actor model, channels, and module layout, see
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md).

## Background

This project exists because the **2025-generation DUPLO trains (10427, and
likely 10428) cannot be controlled by any of the established LEGO BLE
libraries.** I burned considerable time finding this out the hard way.

### What changed in the 2025 hub

The 2025 train hub uses a new **TI CC2642R** BLE controller and **requires
BLE bonding** (LE pairing with persistent keys) before it will accept any
LWP3 command. The older 10874 / 10875 hubs did not.

jncraton's [hardware teardown on Bricks Stack Exchange](https://bricks.stackexchange.com/questions/19118/what-are-the-technical-differences-between-the-2018-and-2025-duplo-train-bases)
identified the CC2642R and is the source of the chipset information used here:

> The primary MCU has been upgraded from a TI CC2640 to a TI CC2642R in the
> 2025 part. Both MCUs are similar, providing 32-bit ARM cores running at
> 48MHz. Bluetooth is also provided by this chip upgrading the supported
> version from 4.2 in the 2018 part to 5.2 in the 2025 part.
>
> ![2018 vs 2025 DUPLO train base — exterior colour comparison](https://i.sstatic.net/V0e0Q8mt.png)
>
> ![2018 vs 2025 DUPLO train base — internal drive mechanism and PCB](https://i.sstatic.net/XX7SFQcg.png)

Existing libraries — [Legoino](https://github.com/corneliusmunz/legoino),
node-poweredup, BrickController2, and similar — target the older hubs and
**skip bonding**. Against a 2025 train, this produces a confusing failure
mode: BLE connects, the LWP3 frames write without errors, and the train
**silently ignores every command**. No error, no notification — just a
connected hub doing nothing.

The community identified the bonding requirement here:

- Legoino issue #90 — <https://github.com/corneliusmunz/legoino/issues/90>
- Brick StackExchange reverse-engineering — <https://bricks.stackexchange.com/questions/18907/functionality-of-new-purple-orange-and-green-duplo-train-action-bricks/18975#18975>

### How this project handles it

The bridge uses [`btleplug`](https://github.com/deviceplug/btleplug) to talk
to the platform BLE stack — **BlueZ** on Linux and **CoreBluetooth** on
macOS — and lets the OS perform the pairing/bonding the hub demands. Once
bonded, the LWP3 motor / LED / sound / speedometer frames documented in
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) work as expected.

If you are working on an ESP32-based controller for the same hub, the same
"bond first, then write LWP3" approach applies — it just has to be done with
a BLE stack that exposes bonding, e.g. NimBLE rather than the libraries that
target the older hubs.

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

## Verify locally

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test --bin duplo-train-controller   # unit tests, no Docker needed
cargo test                                # full suite, requires Docker
```

The full suite spins up a real Mosquitto broker via `testcontainers`. CI runs
the unit-only target.

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

### Commands (subscribe)

**`duplo/train/cmd`**

| Payload | Effect |
|---------|--------|
| `forward` | Drive forward |
| `boost` | Drive forward fast |
| `backward` | Horn + reverse |
| `stop` | Stop motor |

**`duplo/train/led/set`** — Set hub LED colour

`off` · `white` · `green` · `yellow` · `light_blue` · `dark_blue` · `purple` · `purple_pink` · `light_pink` · `red_pink` · `red`

**`duplo/train/sound/set`** — Play train sound

`horn` · `brake` · `steam` · `station_departure` · `water_refill`

### Status (publish)

**`duplo/train/availability`** _(retained)_

`online` when connected · `offline` via Last Will

**`duplo/train/state`** _(retained, JSON)_

```json
{
  "status": "standby|connecting|connected",
  "battery": 85,
  "motor": 50,
  "speed": 42,
  "attempts": 0,
  "ts": 1234567890
}
```

- `status`: `standby` (waiting for command), `connecting` (BLE scan), `connected` (BLE active)
- `motor`: Commanded speed (-100 to 100)
- `speed`: Measured speed from speedometer (only when connected)

**`duplo/train/executed`** _(not retained)_

```json
{"cmd": "forward"}
```

### Test

Broker assumed on `localhost`; add `-h <host>` and `-u/-P` if needed.

```bash
mosquitto_pub -t duplo/train/cmd -m forward      # boost / backward / stop
mosquitto_pub -t duplo/train/led/set -m green    # see colours above
mosquitto_sub -t 'duplo/train/#' -v              # watch all topics
```

## Connection Behavior

See [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) for the BLE state machine and timing details.

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

## Troubleshooting

Quick checks: train awake, BLE up (`bluetoothctl show`), capabilities applied
(`sudo setcap cap_net_raw,cap_net_admin+eip ./duplo-train-controller`).

For the longer list — connection failures, missing speed readings, Docker /
testcontainers errors, cross-compile linker issues — see
[`docs/FAQ.md`](./docs/FAQ.md).

## Documentation

- [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) — actor model, channels, module map.
- [`docs/FAQ.md`](./docs/FAQ.md) — setup, runtime, and development questions.
- [`HOMEASSISTANT.md`](./HOMEASSISTANT.md) — Home Assistant sensors, automations, dashboard.
- [`scripts/README.md`](./scripts/README.md) — Raspberry Pi 4 cross-compile and `systemd` deploy.

## TODO

- [ ] Color sensor readout for custom automations (e.g., trigger actions based on track color tiles)

## License

[MIT](./LICENSE)
