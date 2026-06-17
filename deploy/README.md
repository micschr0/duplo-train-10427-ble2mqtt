# Production Deployment (systemd)

Run the bridge 24/7 on a Raspberry Pi 4 under systemd. `Restart=on-failure`
restarts a crashed actor, so no in-process supervisor is needed.

## 1. Build the release binary

On the Pi:

```bash
cargo build --release
```

Or cross-compile from another machine (see [`../scripts/README.md`](../scripts/README.md)):

```bash
./scripts/build-rpi4.sh
```

## 2. Install the binary

```bash
sudo cp target/release/duplo-train-controller /usr/local/bin/
# cross-compiled: target/aarch64-unknown-linux-gnu/release/duplo-train-controller
```

## 3. Create the config

```bash
sudo mkdir -p /etc/duplo-train
sudo cp .env.example /etc/duplo-train/.env
sudo nano /etc/duplo-train/.env   # set MQTT_HOST, motor speeds, etc.
```

Variables are documented in the [root README](../README.md).

## 4. Install and start the service

```bash
sudo cp deploy/duplo-train-controller.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now duplo-train-controller
```

## 5. Watch the logs

```bash
journalctl -u duplo-train-controller -f
```

## Non-systemd runs

Without systemd, grant the binary BLE capabilities once per release build and
run it directly:

```bash
sudo setcap cap_net_raw,cap_net_admin+eip /usr/local/bin/duplo-train-controller
/usr/local/bin/duplo-train-controller
```

The systemd unit grants these capabilities itself via `AmbientCapabilities`, so
`setcap` is not needed when running under the service.
