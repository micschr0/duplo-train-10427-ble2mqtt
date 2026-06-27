# Architecture

This service is a thin bridge: BLE on one side, MQTT on the other, with a
small protocol layer in between. The two halves run as independent
[tokio](https://tokio.rs) tasks ("actors") that communicate over `mpsc`
channels owned by `main`.

```
                         ┌──────────────────────────────┐
                         │             main             │
                         │  (owns channels + supervisor)│
                         └──────────────┬───────────────┘
                                        │
              cmd_rx                    │                executed_tx
            ┌────────┐ ◀─── Command ────┴──── CommandExecuted ────▶ ┌────────┐
            │ Ble    │ ◀── status_tx ── StatusUpdate ── status_rx ─▶│ Mqtt   │
            │ Actor  │                                              │ Actor  │
            └────┬───┘                                              └────┬───┘
                 │ btleplug                                              │ rumqttc
                 ▼                                                       ▼
       ┌──────────────────┐                                ┌────────────────────┐
       │  DUPLO 10427 hub │                                │  MQTT broker / HA  │
       └──────────────────┘                                └────────────────────┘
```

## Module map

| Path                      | Responsibility                                                                                          |
| ------------------------- | ------------------------------------------------------------------------------------------------------- |
| `src/main.rs`             | Wire up tracing, load config, build channels, spawn the two actors, exit when either one returns.       |
| `src/config.rs`           | `MqttConfig` (`MQTT_*`) and `MotorConfig` (`MOTOR_*`, `BACKWARD_DELAY`) loaded via `serde-env` + `dotenvy`.   |
| `src/types.rs`            | Shared message types between actors: `Command`, `StatusUpdate`, `CommandExecuted`, `TrainState`, enums. |
| `src/ble.rs`              | `BleActor` — adapter scan, connect/disconnect, command writing, notification handling, idle timeouts.   |
| `src/protocol.rs`         | LWP 3.0 constants (UUIDs, ports, message types) plus public re-exports of submodule API.                |
| `src/protocol/buffer.rs`  | Reassembly buffer for fragmented BLE notifications into complete LWP frames.                            |
| `src/protocol/commands.rs`| Encoders for outbound LWP frames (motor, horn, LED, sound, battery request, speedometer subscribe).    |
| `src/protocol/messages.rs`| Decoder for inbound LWP frames into `ParsedMessage` variants.                                           |
| `src/mqtt.rs`             | `MqttActor` — broker connection, retained topic publishing, command parsing, LWT availability.          |
| `tests/mqtt_integration.rs` | End-to-end MQTT tests against a Mosquitto container via `testcontainers`.                              |

## Channels

Three `tokio::sync::mpsc` channels, each with capacity 32 (see
`src/main.rs` constants):

| Channel       | Direction         | Carries           | Purpose                                                            |
| ------------- | ----------------- | ----------------- | ------------------------------------------------------------------ |
| `cmd`         | MQTT → BLE        | `Command`         | High-level intent (`Train`, `Led`, `Sound`).                        |
| `status`      | BLE → MQTT        | `StatusUpdate`    | Connection state, battery, motor, speed, attempts, errors.         |
| `executed`    | BLE → MQTT        | `CommandExecuted` | Confirmation that a motor/horn command finished, for HA feedback.  |

The `Command` enum keeps motor/horn/LED/sound on a single channel so the BLE
actor only has one `select!` source for inbound work.

## BLE actor (`src/ble.rs`)

State machine: `Standby` → `Connecting` → `Connected` and back.

- Lazy connect: stays in `Standby` until the first `Command` arrives.
- Connection attempts retry up to **3** times within a 10 s window; after that
  the actor falls back to `Standby` and emits an error so HA can prompt the
  user to wake the train.
- Idle disconnect after **5 minutes** without a command (saves train battery).
- Periodic ping every **10 seconds** while connected.
- The actor forwards the notification stream into a bounded `mpsc`, where
  `MessageBuffer` reassembles and parses it into `ParsedMessage` variants.
- Status updates are best-effort — a closed `status_tx` logs a warning; the loop
  exits on its next `cmd_rx` poll.

Boost can optionally auto-revert to `Forward` after `MOTOR_BOOST_DURATION`
seconds; the actor tracks `boost_expires` and races a timer in its `select!`.

## MQTT actor (`src/mqtt.rs`)

- Topics are derived from `MQTT_BASE_TOPIC` (`<base>/cmd`, `<base>/led/set`,
  `<base>/sound/set`, `<base>/state`, `<base>/executed`,
  `<base>/availability`).
- Last-Will publishes `offline` to `<base>/availability` (retained, QoS 1) so
  Home Assistant sees disconnects immediately.
- On `ConnAck`, publishes `online` and re-subscribes to the command topics.
- Transient `event_loop.poll()` errors trigger a 5 s backoff while the actor
  continues draining `status_rx` so the BLE actor never blocks on MQTT
  hiccups.
- The actor holds `TrainState`; updates merge in — battery, LED, and last sound
  survive standby/connect transitions. It republishes the full state to
  `<base>/state` (retained) on every change.

## Supervision

`main` joins both actors via `tokio::select!`. If either task returns, the
process exits with the captured error; there is no in-process restart loop.
Production deployments rely on `systemd` (`Restart=always`) — see
[`scripts/README.md`](../scripts/README.md) for the unit file template.

## What this design deliberately doesn't do

- No persistence. State is reconstructed from the train on reconnect; nothing
  is written to disk.
- No multi-train support. The bridge talks to the first hub it finds; running
  more than one train means running more than one bridge process with
  distinct `MQTT_CLIENT_ID` and `MQTT_BASE_TOPIC` values.
- No TLS to MQTT. `rumqttc` is built with `default-features = false`; users
  who need TLS-protected brokers should either tunnel locally or extend the
  build.
