# DUPLO Train Controller

BLE-to-MQTT bridge for LEGO DUPLO 10427 trains. Runs on Raspberry Pi 4, integrates with Home Assistant.

## Commands

```bash
cargo build                   # compile
cargo test                    # unit + integration tests (Docker required for integration tests)
cargo fmt                     # format
cargo clippy                  # lint
cargo run                     # run (needs MQTT broker + BLE adapter)
./scripts/build-rpi4.sh       # cross-compile for RPi4 (needs `cross` and Docker)
```

## Architecture

Two async actors communicate via tokio mpsc channels:

- **BleActor** (`src/ble.rs`) — scans for the train, sends BLE commands, reads status updates
- **MqttActor** (`src/mqtt.rs`) — publishes train status, subscribes to incoming commands
- **Protocol** (`src/protocol.rs`) — LWP wire format: message encoding/decoding, BLE buffer handling
- **Config** (in `src/mqtt.rs`) — `MqttConfig` (env prefix `MQTT_`) and `MotorConfig` (no prefix), loaded via `envy` + `dotenvy`

MQTT topic layout (`<MQTT_BASE_TOPIC>/state`, `/executed`, `/availability`, `/cmd`) and Home Assistant integration are documented in `HOMEASSISTANT.md`.

## Environment Variables

Create a `.env` file (auto-loaded via `dotenvy`):

| Variable | Default | Description |
|----------|---------|-------------|
| `MQTT_HOST` | `localhost` | MQTT broker host |
| `MQTT_PORT` | `1883` | MQTT broker port |
| `MQTT_USERNAME` | — | Optional |
| `MQTT_PASSWORD` | — | Optional |
| `MQTT_CLIENT_ID` | `duplo-train-gateway` | |
| `MQTT_BASE_TOPIC` | `duplo/train` | |
| `MOTOR_FORWARD` | `50` | Forward speed 1–100 |
| `MOTOR_BOOST` | `75` | Boost speed 1–100 |
| `MOTOR_BOOST_DURATION` | — | Auto-revert boost after N seconds (0 = unlimited) |
| `MOTOR_BACKWARD` | `-50` | Backward speed -100 to -1 |
| `BACKWARD_DELAY` | `1200` | Delay before backward motion (ms) |
| `RUST_LOG` | `info,duplo_train_controller=debug` | Log filter |

Motor speeds are validated at startup — invalid ranges cause a fatal error.

## Gotchas

- **Linux BLE permissions:** Binary needs capabilities after each release build:
  ```bash
  sudo setcap cap_net_raw,cap_net_admin+eip ./target/release/duplo-train-controller
  ```
- **Integration tests need Docker:** `tests/mqtt_integration.rs` spins up a real Mosquitto broker via testcontainers — Docker must be running or those tests fail.
- **Run unit tests without Docker:** `cargo test --bins` skips the Mosquitto-dependent integration suite (133 unit tests in `src/`).
- **Cross-compile needs `cross`:** `./scripts/build-rpi4.sh` uses cross-rs; install with `cargo install cross --git https://github.com/cross-rs/cross`.
