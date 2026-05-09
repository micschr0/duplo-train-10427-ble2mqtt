# Build Scripts

## Cross-Compilation for Raspberry Pi 4

### Prerequisites

```bash
# Install cross-rs
cargo install cross --git https://github.com/cross-rs/cross

# Docker must be running
docker info
```

### Build

```bash
# Release build (optimized, small)
./scripts/build-rpi4.sh

# Debug build (faster compilation, larger binary)
./scripts/build-rpi4.sh debug
```

### Output

```
target/aarch64-unknown-linux-gnu/release/duplo-train-controller
```

### Deployment to RPi4

```bash
# Copy binary
scp target/aarch64-unknown-linux-gnu/release/duplo-train-controller pi@<rpi-ip>:~/

# Copy .env file (adjust as needed!)
scp .env.example pi@<rpi-ip>:~/.env
```

### On the RPi4

```bash
# Set Bluetooth permissions (alternatively: run as root)
sudo setcap cap_net_raw,cap_net_admin+eip ./duplo-train-controller

# Run
RUST_LOG=info ./duplo-train-controller
```

### Systemd Service (optional)

```bash
sudo nano /etc/systemd/system/duplo-train.service
```

```ini
[Unit]
Description=DUPLO Train BLE-MQTT Gateway
After=network.target bluetooth.target

[Service]
Type=simple
User=pi
WorkingDirectory=/home/pi
EnvironmentFile=/home/pi/.env
ExecStart=/home/pi/duplo-train-controller
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable duplo-train
sudo systemctl start duplo-train

# View logs
journalctl -u duplo-train -f
```
