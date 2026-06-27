//! BLE actor for LEGO DUPLO train communication.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter,
    ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};

use crate::config::MotorConfig;
use crate::protocol::{
    self, CHARACTERISTIC_UUID, CommandFeedback, DeviceType, IoEvent, MANUFACTURER_ID,
    MessageBuffer, ParsedMessage, SERVICE_UUID,
};
use crate::types::{
    Command, CommandExecuted, ConnectionState, StatusUpdate, TrainCommand, TrainState,
};

const IDLE_DISCONNECT_MS: u64 = 300_000; // 5 minutes
const PING_INTERVAL_MS: u64 = 10_000; // 10 seconds
const SCAN_TIMEOUT_MS: u64 = 30_000; // 30 seconds
const ATTEMPT_WINDOW_MS: u64 = 10_000; // 10 seconds window for attempt counting

/// Send a status update; warn if the receiver has gone away.
///
/// Status updates are best-effort — losing one is not fatal, but a closed
/// channel means the MQTT actor is gone and the BLE loop will exit on its
/// next `cmd_rx` poll. The warning gives visibility while that happens.
async fn notify(tx: &mpsc::Sender<StatusUpdate>, update: StatusUpdate) {
    if tx.send(update).await.is_err() {
        warn!("status channel closed; MQTT actor gone?");
    }
}

/// Compute the next attempt number from the previous count and how long ago
/// the last attempt occurred. Returns 1 if the window has expired or there
/// was no prior attempt; otherwise increments and clamps to 3.
fn classify_attempt(prev_attempts: u8, last_elapsed: Option<Duration>) -> u8 {
    match last_elapsed {
        Some(elapsed) if elapsed < Duration::from_millis(ATTEMPT_WINDOW_MS) => {
            (prev_attempts + 1).min(3)
        }
        _ => 1,
    }
}

/// BLE actor for train communication.
#[must_use = "BLE actor must be passed to run()"]
pub struct BleActor {
    adapter: Adapter,
    peripheral: Option<Peripheral>,
    characteristic: Option<Characteristic>,
    notification_rx: Option<mpsc::Receiver<ValueNotification>>,
    notification_task: Option<JoinHandle<()>>,
    last_command: Instant,
    last_ping: Instant,
    connection_state: ConnectionState,
    attempts: u8,
    last_attempt: Option<Instant>,
    message_buffer: MessageBuffer,
    last_battery: Option<u8>,
    last_speed: i16,
    boost_expires: Option<tokio::time::Instant>,
}

impl BleActor {
    /// Create a new BLE actor.
    pub async fn new() -> Result<Self> {
        let manager = Manager::new()
            .await
            .context("Failed to create BLE manager")?;
        let adapters = manager
            .adapters()
            .await
            .context("Failed to get BLE adapters")?;
        let adapter = adapters
            .into_iter()
            .next()
            .context("No BLE adapters found")?;

        info!(adapter = ?adapter.adapter_info().await?, "BLE adapter initialized");

        Ok(Self {
            adapter,
            peripheral: None,
            characteristic: None,
            notification_rx: None,
            notification_task: None,
            last_command: Instant::now(),
            last_ping: Instant::now(),
            connection_state: ConnectionState::Standby,
            attempts: 0,
            last_attempt: None,
            message_buffer: MessageBuffer::new(),
            last_battery: None,
            last_speed: 0,
            boost_expires: None,
        })
    }

    /// Run the BLE actor event loop.
    pub async fn run(
        mut self,
        mut cmd_rx: mpsc::Receiver<Command>,
        status_tx: mpsc::Sender<StatusUpdate>,
        executed_tx: mpsc::Sender<CommandExecuted>,
        motor_config: MotorConfig,
    ) -> Result<()> {
        info!("BLE actor started");

        let mut ping_interval = tokio::time::interval(Duration::from_millis(PING_INTERVAL_MS));
        let mut idle_check = tokio::time::interval(Duration::from_secs(60));

        loop {
            tokio::select! {
                biased;

                recv = cmd_rx.recv() => {
                    let Some(cmd) = recv else {
                        info!("Command channel closed, BLE actor shutting down");
                        break;
                    };
                    self.last_command = Instant::now();
                    self.handle_command(cmd, &status_tx, &executed_tx, &motor_config).await;
                }

                Some(notification) = async {
                    match self.notification_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.handle_notification(&notification, &status_tx).await;
                }

                // sleep_until with a fixed deadline is used (not sleep with a
                // remaining duration) so a high-frequency notification stream
                // cannot starve boost expiry by resetting the timer each iteration.
                _ = async {
                    match self.boost_expires {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                } => {
                    if self.connection_state == ConnectionState::Connected {
                        info!("Boost duration expired, reverting to forward speed");
                        self.boost_expires = None;
                        let speed = motor_config.forward;
                        if let Err(e) = self.write_command(&protocol::motor_command(speed)).await {
                            error!(error = %e, "Failed to revert from boost to forward");
                        } else {
                            notify(&status_tx, StatusUpdate::Motor(speed)).await;
                        }
                    } else {
                        self.boost_expires = None;
                    }
                }

                _ = ping_interval.tick() => {
                    if self.connection_state == ConnectionState::Connected {
                        if !self.is_connected().await {
                            warn!("BLE connection lost (detected via ping check)");
                            self.handle_disconnect(&status_tx).await;
                        } else if self.last_ping.elapsed() > Duration::from_millis(PING_INTERVAL_MS) {
                            if let Err(e) = self.send_battery_request().await {
                                debug!(error = %e, "Battery request failed");
                                if !self.is_connected().await {
                                    warn!("BLE connection lost after failed ping");
                                    self.handle_disconnect(&status_tx).await;
                                }
                            }
                            self.last_ping = Instant::now();
                        }
                    }
                }

                _ = idle_check.tick() => {
                    if self.connection_state == ConnectionState::Connected
                        && self.last_command.elapsed() > Duration::from_millis(IDLE_DISCONNECT_MS)
                    {
                        info!("Idle timeout, disconnecting");
                        self.handle_disconnect(&status_tx).await;
                    }
                }
            }
        }

        if let Err(e) = self.disconnect().await {
            warn!(error = %e, "Disconnect during shutdown failed");
        }
        Ok(())
    }

    /// Handle an incoming command.
    async fn handle_command(
        &mut self,
        cmd: Command,
        status_tx: &mpsc::Sender<StatusUpdate>,
        executed_tx: &mpsc::Sender<CommandExecuted>,
        motor_config: &MotorConfig,
    ) {
        if self.connection_state != ConnectionState::Connected {
            self.attempts = classify_attempt(self.attempts, self.last_attempt.map(|i| i.elapsed()));
            self.last_attempt = Some(Instant::now());

            info!(
                ?cmd,
                attempts = self.attempts,
                "Wake-up command received, starting scan"
            );

            self.connection_state = ConnectionState::Connecting;
            notify(
                status_tx,
                StatusUpdate::ConnectionState(ConnectionState::Connecting),
            )
            .await;
            notify(status_tx, StatusUpdate::Attempts(self.attempts)).await;

            // Exponential backoff: no delay on first attempt, 2 s on second,
            // 8 s on third and beyond — prevents hammering the adapter when
            // the train is out of range and HA keeps sending commands.
            let backoff = match self.attempts {
                1 => Duration::ZERO,
                2 => Duration::from_secs(2),
                _ => Duration::from_secs(8),
            };
            if !backoff.is_zero() {
                info!(
                    backoff_secs = backoff.as_secs(),
                    "Backing off before BLE scan"
                );
                tokio::time::sleep(backoff).await;
            }

            match self.scan_and_connect().await {
                Ok(()) => {
                    self.connection_state = ConnectionState::Connected;
                    self.attempts = 0;
                    self.last_attempt = None;
                    notify(
                        status_tx,
                        StatusUpdate::ConnectionState(ConnectionState::Connected),
                    )
                    .await;
                    notify(status_tx, StatusUpdate::Attempts(0)).await;
                    notify(status_tx, StatusUpdate::State(TrainState::connected())).await;
                    info!("Connected to train");

                    if let Err(e) = self.send_battery_request().await {
                        debug!(error = %e, "Initial battery request failed");
                    }
                }
                Err(e) => {
                    error!(error = %e, attempts = self.attempts, "Failed to connect");
                    self.connection_state = ConnectionState::Standby;
                    notify(status_tx, StatusUpdate::Error(e.to_string())).await;
                    notify(
                        status_tx,
                        StatusUpdate::ConnectionState(ConnectionState::Standby),
                    )
                    .await;
                    // Keep attempts count for HA feedback — don't reset on failure
                }
            }
            // Wake-up command is NOT executed - user must send another command
            return;
        }

        match self.execute_command(cmd, status_tx, motor_config).await {
            Ok(()) => {
                // Only motor/horn commands surface on the executed topic;
                // LED/sound state is already observable via TrainState.
                if let Command::Train(t) = cmd
                    && executed_tx.send(CommandExecuted { cmd: t }).await.is_err()
                {
                    warn!("executed channel closed; MQTT actor gone?");
                }
            }
            Err(e) => {
                error!(error = %e, "Command execution failed");
                notify(status_tx, StatusUpdate::Error(e.to_string())).await;

                if !self.is_connected().await {
                    self.handle_disconnect(status_tx).await;
                }
            }
        }
    }

    /// Scan for and connect to the DUPLO train (synchronous/blocking scan).
    async fn scan_and_connect(&mut self) -> Result<()> {
        info!("Scanning for DUPLO train...");

        self.adapter
            .start_scan(ScanFilter::default())
            .await
            .context("Failed to start scan")?;

        // Get FRESH event stream AFTER starting scan
        let mut events = self
            .adapter
            .events()
            .await
            .context("Failed to get BLE events")?;

        let scan_start = Instant::now();

        if let Some(peripheral) = self.find_train_in_peripherals().await {
            info!("Found DUPLO train in cached peripherals");
            if let Err(e) = self.adapter.stop_scan().await {
                debug!(error = %e, "stop_scan failed");
            }
            return self.connect_to_peripheral(peripheral).await;
        }

        while scan_start.elapsed() < Duration::from_millis(SCAN_TIMEOUT_MS) {
            let timeout_remaining =
                Duration::from_millis(SCAN_TIMEOUT_MS).saturating_sub(scan_start.elapsed());

            match tokio::time::timeout(timeout_remaining, events.next()).await {
                Ok(Some(CentralEvent::DeviceDiscovered(id))) => {
                    debug!(?id, "Device discovered");
                    if let Ok(peripherals) = self.adapter.peripherals().await {
                        for peripheral in peripherals {
                            if peripheral.id() == id && self.is_duplo_train(&peripheral).await {
                                info!("Found DUPLO train via discovery event");
                                if let Err(e) = self.adapter.stop_scan().await {
                                    debug!(error = %e, "stop_scan failed");
                                }
                                return self.connect_to_peripheral(peripheral).await;
                            }
                        }
                    }
                }
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => {
                    debug!("Scan timeout, checking peripherals one last time");
                    if let Some(peripheral) = self.find_train_in_peripherals().await {
                        info!("Found DUPLO train on final check");
                        if let Err(e) = self.adapter.stop_scan().await {
                            debug!(error = %e, "stop_scan failed");
                        }
                        return self.connect_to_peripheral(peripheral).await;
                    }
                    break;
                }
            }
        }

        if let Err(e) = self.adapter.stop_scan().await {
            debug!(error = %e, "stop_scan failed");
        }
        anyhow::bail!("Scan timeout: DUPLO train not found")
    }

    /// Find DUPLO train in already-discovered peripherals.
    async fn find_train_in_peripherals(&self) -> Option<Peripheral> {
        let peripherals = self.adapter.peripherals().await.ok()?;
        for peripheral in peripherals {
            if self.is_duplo_train(&peripheral).await {
                return Some(peripheral);
            }
        }
        None
    }

    /// Connect to a specific peripheral.
    async fn connect_to_peripheral(&mut self, peripheral: Peripheral) -> Result<()> {
        info!("Connecting to DUPLO train...");

        peripheral.connect().await.context("Failed to connect")?;

        let (characteristic, notification_rx, notification_task) =
            self.setup_peripheral(&peripheral).await?;

        if let Some(prev) = self.notification_task.take() {
            prev.abort();
        }

        self.peripheral = Some(peripheral);
        self.characteristic = Some(characteristic);
        self.notification_rx = Some(notification_rx);
        self.notification_task = Some(notification_task);
        self.last_command = Instant::now();
        self.last_ping = Instant::now();

        Ok(())
    }

    /// Execute a command on the connected train.
    async fn execute_command(
        &mut self,
        cmd: Command,
        status_tx: &mpsc::Sender<StatusUpdate>,
        motor_config: &MotorConfig,
    ) -> Result<()> {
        match cmd {
            Command::Train(t) => self.execute_train_command(t, status_tx, motor_config).await,
            Command::Led(color) => {
                self.write_command(&protocol::led_color_command(color.into()))
                    .await?;
                notify(status_tx, StatusUpdate::Led(color)).await;
                Ok(())
            }
            Command::Sound(sound) => {
                self.write_command(&protocol::sound_packet(sound.into()))
                    .await?;
                notify(status_tx, StatusUpdate::Sound(sound)).await;
                Ok(())
            }
        }
    }

    /// Execute a motor/horn command variant.
    async fn execute_train_command(
        &mut self,
        cmd: TrainCommand,
        status_tx: &mpsc::Sender<StatusUpdate>,
        motor_config: &MotorConfig,
    ) -> Result<()> {
        match cmd {
            TrainCommand::Forward => {
                self.boost_expires = None;
                let speed = motor_config.forward;
                self.write_command(&protocol::motor_command(speed)).await?;
                notify(status_tx, StatusUpdate::Motor(speed)).await;
            }
            TrainCommand::Boost => {
                let speed = motor_config.boost;
                self.write_command(&protocol::motor_command(speed)).await?;
                notify(status_tx, StatusUpdate::Motor(speed)).await;

                if let Some(duration_secs) = motor_config.boost_duration {
                    self.boost_expires =
                        Some(tokio::time::Instant::now() + Duration::from_secs(duration_secs));
                    debug!(duration_secs, "Boost timer started");
                } else {
                    self.boost_expires = None;
                }
            }
            TrainCommand::Backward => {
                self.boost_expires = None;

                // Backward sequence: stop → delay → backward (horn during delay)
                self.write_command(&protocol::motor_command(0)).await?;

                // Horn (ignore errors - not critical)
                if let Err(e) = self.write_command(&protocol::horn_command()).await {
                    debug!(error = %e, "Horn command failed, continuing");
                }

                tokio::time::sleep(Duration::from_millis(motor_config.backward_delay)).await;

                if !self.is_connected().await {
                    notify(status_tx, StatusUpdate::Motor(0)).await;
                    anyhow::bail!("Connection lost during backward sequence");
                }

                let speed = motor_config.backward;
                self.write_command(&protocol::motor_command(speed)).await?;
                notify(status_tx, StatusUpdate::Motor(speed)).await;
            }
            TrainCommand::Stop => {
                self.boost_expires = None;
                self.write_command(&protocol::motor_command(0)).await?;
                notify(status_tx, StatusUpdate::Motor(0)).await;
            }
        }
        Ok(())
    }

    /// Handle a BLE notification from the train.
    async fn handle_notification(
        &mut self,
        notification: &ValueNotification,
        status_tx: &mpsc::Sender<StatusUpdate>,
    ) {
        debug!(data = ?notification.value, "Received BLE notification");

        let messages = self.message_buffer.push(&notification.value);

        for message in messages {
            if let Some(parsed) = protocol::parse_message(&message) {
                self.handle_parsed_message(parsed, status_tx).await;
            }
        }
    }

    /// Handle a parsed message from the train.
    async fn handle_parsed_message(
        &mut self,
        message: ParsedMessage,
        status_tx: &mpsc::Sender<StatusUpdate>,
    ) {
        match message {
            ParsedMessage::Battery(battery_pct) => {
                // Only accept battery readings when train is stationary (voltage drops under load)
                if self.last_speed != 0 {
                    debug!(
                        battery_pct,
                        speed = self.last_speed,
                        "Ignoring battery reading (train moving)"
                    );
                    return;
                }

                if battery_pct > 100 {
                    warn!(
                        battery_pct,
                        "Hub reported out-of-range battery level; ignoring"
                    );
                } else if self.last_battery != Some(battery_pct) {
                    self.last_battery = Some(battery_pct);
                    debug!(battery_pct, "Battery level updated");
                    notify(status_tx, StatusUpdate::Battery(battery_pct)).await;
                }
            }
            ParsedMessage::HubAttachedIo {
                port_id,
                event,
                device_type,
            } => match event {
                IoEvent::Attached | IoEvent::AttachedVirtual => {
                    if let Some(ref dtype) = device_type {
                        info!(
                            port_id = format!("0x{:02X}", port_id),
                            device = dtype.name(),
                            device_id = match dtype {
                                DeviceType::Unknown(id) => format!("0x{:04X}", id),
                                _ => String::new(),
                            },
                            "Device attached"
                        );

                        // Auto-subscribe to speedometer when attached
                        if matches!(dtype, DeviceType::DuploTrainSpeedometer)
                            && let Err(e) = self.subscribe_speedometer(port_id).await
                        {
                            warn!(error = %e, "Failed to subscribe to speedometer");
                        }
                    } else {
                        info!(
                            port_id = format!("0x{:02X}", port_id),
                            "Device attached (unknown type)"
                        );
                    }
                }
                IoEvent::Detached => {
                    info!(port_id = format!("0x{:02X}", port_id), "Device detached");
                }
            },
            ParsedMessage::Speedometer(speed) => {
                self.last_speed = speed;
                debug!(speed, "Speedometer reading");
                notify(status_tx, StatusUpdate::Speed(speed)).await;
            }
            ParsedMessage::Feedback { port_id, feedback } => {
                debug!(?port_id, ?feedback, "Command feedback received");
                if let CommandFeedback::Discarded = feedback {
                    warn!(port_id, "Command was discarded by hub");
                }
            }
            ParsedMessage::Unknown { msg_type, data } => {
                debug!(msg_type, ?data, "Unknown message type");
            }
        }
    }

    /// Handle disconnection (cleanup and notify).
    async fn handle_disconnect(&mut self, status_tx: &mpsc::Sender<StatusUpdate>) {
        if let Err(e) = self.disconnect().await {
            warn!(error = %e, "Disconnect cleanup failed");
        }

        self.connection_state = ConnectionState::Standby;
        self.attempts = 0;
        self.last_attempt = None;
        self.message_buffer.clear();
        self.boost_expires = None;

        notify(
            status_tx,
            StatusUpdate::ConnectionState(ConnectionState::Standby),
        )
        .await;
        notify(status_tx, StatusUpdate::Attempts(0)).await;
        notify(status_tx, StatusUpdate::State(TrainState::standby())).await;
    }

    /// Check if currently connected.
    async fn is_connected(&self) -> bool {
        match &self.peripheral {
            Some(p) => p.is_connected().await.unwrap_or(false),
            None => false,
        }
    }

    /// Check if a peripheral is a DUPLO train.
    async fn is_duplo_train(&self, peripheral: &Peripheral) -> bool {
        let Ok(Some(props)) = peripheral.properties().await else {
            return false;
        };

        if props.manufacturer_data.contains_key(&MANUFACTURER_ID) {
            debug!(name = ?props.local_name, "Found LEGO device");
            return true;
        }

        if props.services.contains(&SERVICE_UUID) {
            debug!(name = ?props.local_name, "Found device with DUPLO service UUID");
            return true;
        }

        false
    }

    /// Setup peripheral after connection (discover services, subscribe to notifications).
    async fn setup_peripheral(
        &self,
        peripheral: &Peripheral,
    ) -> Result<(
        Characteristic,
        mpsc::Receiver<ValueNotification>,
        JoinHandle<()>,
    )> {
        peripheral
            .discover_services()
            .await
            .context("Failed to discover services")?;

        let characteristic = peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == CHARACTERISTIC_UUID)
            .context("DUPLO characteristic not found")?;

        // Get notification stream BEFORE subscribing (btleplug/BlueZ quirk)
        let mut notification_stream = peripheral
            .notifications()
            .await
            .context("Failed to get notification stream")?;

        peripheral
            .subscribe(&characteristic)
            .await
            .context("Failed to subscribe to notifications")?;

        // Additional delay after subscribe for BlueZ
        #[cfg(target_os = "linux")]
        tokio::time::sleep(Duration::from_millis(250)).await;

        let (tx, rx) = mpsc::channel(32);

        let task = tokio::spawn(async move {
            while let Some(n) = notification_stream.next().await {
                if tx.send(n).await.is_err() {
                    break;
                }
            }
        });

        debug!("Peripheral setup complete");
        Ok((characteristic, rx, task))
    }

    /// Write a command to the train.
    async fn write_command(&self, data: &[u8]) -> Result<()> {
        let peripheral = self.peripheral.as_ref().context("Not connected")?;
        let characteristic = self.characteristic.as_ref().context("No characteristic")?;

        peripheral
            .write(characteristic, data, WriteType::WithoutResponse)
            .await
            .context("Failed to write command")?;

        debug!(data = ?data, "Wrote command");
        Ok(())
    }

    /// Send battery request (keep-alive).
    async fn send_battery_request(&self) -> Result<()> {
        self.write_command(&protocol::battery_request()).await
    }

    /// Subscribe to speedometer notifications.
    async fn subscribe_speedometer(&self, port_id: u8) -> Result<()> {
        info!(
            port_id = format!("0x{:02X}", port_id),
            "Subscribing to speedometer"
        );
        self.write_command(&protocol::speedometer_subscribe(port_id, 1))
            .await
    }

    /// Disconnect from the train.
    async fn disconnect(&mut self) -> Result<()> {
        if let Some(task) = self.notification_task.take() {
            task.abort();
        }
        if let Some(ref peripheral) = self.peripheral
            && peripheral.is_connected().await.unwrap_or(false)
        {
            peripheral.disconnect().await?;
        }
        self.peripheral = None;
        self.characteristic = None;
        self.notification_rx = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod connection_state {
        use super::*;

        #[test]
        fn default_is_standby() {
            assert_eq!(ConnectionState::default(), ConnectionState::Standby);
        }
    }

    mod timing_constants {
        use super::*;

        #[test]
        fn idle_disconnect_is_5_minutes() {
            assert_eq!(IDLE_DISCONNECT_MS, 300_000);
        }

        #[test]
        fn ping_interval_is_10_seconds() {
            assert_eq!(PING_INTERVAL_MS, 10_000);
        }

        #[test]
        fn scan_timeout_is_30_seconds() {
            assert_eq!(SCAN_TIMEOUT_MS, 30_000);
        }
    }

    mod attempts_counting {
        use super::*;

        fn ms(v: u64) -> Option<Duration> {
            Some(Duration::from_millis(v))
        }

        #[test]
        fn first_attempt_is_1() {
            assert_eq!(classify_attempt(0, None), 1);
        }

        #[test]
        fn second_attempt_within_window_is_2() {
            assert_eq!(classify_attempt(1, ms(5_000)), 2);
        }

        #[test]
        fn third_attempt_within_window_is_3() {
            assert_eq!(classify_attempt(2, ms(5_000)), 3);
        }

        #[test]
        fn attempts_cap_at_3() {
            assert_eq!(classify_attempt(3, ms(5_000)), 3);
            assert_eq!(classify_attempt(10, ms(5_000)), 3);
        }

        #[test]
        fn attempt_after_window_expires_resets_to_1() {
            assert_eq!(classify_attempt(2, ms(15_000)), 1);
            assert_eq!(classify_attempt(3, ms(10_001)), 1);
        }

        #[test]
        fn attempt_at_exactly_window_boundary_resets() {
            assert_eq!(classify_attempt(2, ms(10_000)), 1);
        }

        #[test]
        fn attempt_just_under_window_increments() {
            assert_eq!(classify_attempt(2, ms(9_999)), 3);
        }

        #[test]
        fn attempt_window_constant_is_10_seconds() {
            assert_eq!(ATTEMPT_WINDOW_MS, 10_000);
        }

        #[test]
        fn attempts_sequence_for_ha_feedback() {
            // - attempts 0→1: bell sound
            // - attempts 1→2: voice hint
            // - attempts 2→3: error sound
            let mut attempts = 0u8;
            attempts = classify_attempt(attempts, None);
            assert_eq!(attempts, 1);
            attempts = classify_attempt(attempts, ms(5_000));
            assert_eq!(attempts, 2);
            attempts = classify_attempt(attempts, ms(5_000));
            assert_eq!(attempts, 3);
        }
    }
}
