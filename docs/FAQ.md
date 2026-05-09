# FAQ

A loose collection of questions that come up often enough to be worth writing
down. If you hit something that should live here, please open an issue.

## Setup

### Which trains are supported?

Tested on the **LEGO DUPLO 10427 Steam Train** with the original firmware. The
LEGO Wireless Protocol 3.0 frames used here are the same family as on other
recent DUPLO hubs, so newer DUPLO trains likely work too — but I haven't
verified that.

In particular, the **LEGO DUPLO 10428 (Cargo Train)** reportedly uses the
same LWP3 hub family, so the bridge should work with it without code changes.
I haven't personally tested it. If you try the 10428 (or any other DUPLO
train), please file an issue with the result either way — I'll update this
list.

**Older DUPLO trains (10874, 10875) are not supported.** They predate the
2025 hub's BLE bonding requirement and are well covered by
[Legoino](https://github.com/corneliusmunz/legoino) and similar libraries;
see the [README's Background section](../README.md#background) for the long
story.

### Which platforms run the bridge?

I've verified two host platforms end-to-end against a real DUPLO 10427 hub:

- **Raspberry Pi 4 (aarch64, 64-bit Raspberry Pi OS)** — the production
  target I deploy to.
- **macOS (Apple Silicon)** — used both for development and for running the
  bridge directly. The native `btleplug` backend talks to CoreBluetooth, so
  no extra setcap / capability dance is required; on first launch macOS will
  prompt for Bluetooth permission for the binary.

Any other Linux host with a working BLE stack and `bluetoothd` should also
work; I haven't tested it. Windows is untested.

### Why does it need MQTT?

This is a bridge, not a controller. It exposes the train as MQTT topics so
Home Assistant (or any other MQTT client) can drive it from automations, voice
assistants, dashboards, or Zigbee buttons. If you do not already have an MQTT
broker, **Mosquitto** is the simplest one to install — Home Assistant also
ships an MQTT add-on that works out of the box.

## Running the bridge

### `Failed to get BLE adapters` / "no adapters" on Linux

Either the kernel BLE stack is asleep or the binary lacks capabilities:

```bash
# Check the adapter is up
bluetoothctl show

# Grant capabilities so the binary can talk to BLE without sudo
sudo setcap cap_net_raw,cap_net_admin+eip ./duplo-train-controller
```

If `setcap` was applied, the executable has been rebuilt, and capabilities are
gone again — that is expected. Re-run `setcap` after every release build.

### The train is not found during scan

Most often the hub is asleep. Press the button on the train to wake it up;
the green LED should be steady. If a scan still returns nothing:

```bash
# Confirm BlueZ can see the device at all
sudo bluetoothctl scan on

# Power-cycle the local adapter
sudo systemctl restart bluetooth
```

### Commands are sent but the train does not move

Look in the log for a `Device attached` line shortly after connection. If
it never appears, notifications are probably not flowing — restart the
service and check `RUST_LOG=debug` output. A still-paused train, low
batteries, or a different hub responding on the same characteristic UUID can
all produce this symptom.

### Connecting takes forever and then fails

The actor retries connection three times in a 10 s window before backing off
to `standby`. Three attempts that all fail typically mean the train is asleep
or out of range. Wake it up and send a command again; the bridge connects
lazily on demand.

### MQTT shows `availability: offline` even though the bridge is running

Two common causes:

1. The bridge is connected to the broker but lost it after the initial
   `online` publish — Last-Will pushed `offline` to the retained topic. Check
   broker logs and network connectivity, then restart the bridge.
2. The retained `offline` from a previous run is being read by Home Assistant
   on cold start. Restart the bridge once and a fresh `online` will replace
   it.

### Speed reads zero forever

The speedometer port id differs between hub revisions; both `0x36` and `0x33`
are recognized. If your hub uses something else the speedometer subscribe
will succeed but no values will arrive. Open an issue with a `RUST_LOG=debug`
log of the connection — the `IoEvent::Attached` line will show the port id
the hub announces.

## Development

### Tests fail with "Cannot connect to the Docker daemon"

The full suite includes integration tests that boot a Mosquitto container via
[`testcontainers`](https://docs.rs/testcontainers). If you do not have Docker
running, run the unit suite only:

```bash
cargo test --bin duplo-train-controller
```

CI uses the same target so PRs do not need Docker either.

### Cross-compile to Raspberry Pi 4 fails on linker errors

The `./scripts/build-rpi4.sh` helper requires
[`cross`](https://github.com/cross-rs/cross) and a working Docker daemon —
`cross` invokes a Linux container with the right toolchain, so building it
without Docker will not work. Install `cross` with:

```bash
cargo install cross --git https://github.com/cross-rs/cross
```

### Why is `cargo audit` ignoring two advisories?

`RUSTSEC-2025-0111` (`tokio-tar`) and `RUSTSEC-2025-0134` (`rustls-pemfile`)
both come in via `testcontainers` and only affect the test binary, not the
released crate. They are explicitly ignored in the CI workflow with a comment
linking back to the upstream status. If a fix becomes available the ignore
should be removed.

### Can I add support for sound X / LED color Y / behaviour Z?

Probably yes if the train firmware exposes it — see
[`docs/ARCHITECTURE.md`](./ARCHITECTURE.md) for where commands are encoded
(`src/protocol/commands.rs`) and parsed (`src/protocol/messages.rs`). Open an
issue first so the scope is agreed before you start writing code.

## Project status

### Will this become a polished product?

No. This is a hobby project I wrote to control my kid's DUPLO train and to
learn Rust. Expect commits to be sporadic. Issues and PRs are welcome —
keep the scope modest, see [`CONTRIBUTING.md`](../CONTRIBUTING.md).
