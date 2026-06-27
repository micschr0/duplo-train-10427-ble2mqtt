//! MQTT actor for Home Assistant integration.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::sync::mpsc;
use tokio::time::{Instant as TokioInstant, sleep_until};
use tracing::{debug, error, info, warn};

/// Backoff after a transient MQTT event-loop error before resuming polling.
/// Status drain continues during this window so the BLE actor isn't blocked.
const MQTT_ERROR_BACKOFF: Duration = Duration::from_secs(5);

use crate::config::MqttConfig;
use crate::types::{
    Command, CommandExecuted, ConnectionState, DuploSound, LedColor, StatusUpdate, TrainCommand,
    TrainState,
};

/// Runtime-constructed MQTT topics.
#[derive(Debug, Clone)]
pub struct Topics {
    pub cmd: String,
    pub led_set: String,
    pub sound_set: String,
    pub state: String,
    pub executed: String,
    pub availability: String,
}

impl Topics {
    fn new(base_topic: &str) -> Self {
        Self {
            cmd: format!("{}/cmd", base_topic),
            led_set: format!("{}/led/set", base_topic),
            sound_set: format!("{}/sound/set", base_topic),
            state: format!("{}/state", base_topic),
            executed: format!("{}/executed", base_topic),
            availability: format!("{}/availability", base_topic),
        }
    }
}

/// MQTT actor for Home Assistant communication.
pub struct MqttActor {
    client: AsyncClient,
    topics: Topics,
    current_state: TrainState,
    last_cmd: Option<TrainCommand>,
    last_cmd_time: Instant,
}

impl MqttActor {
    /// Create a new MQTT actor.
    pub async fn new(config: MqttConfig) -> Result<(Self, rumqttc::EventLoop)> {
        let topics = Topics::new(&config.base_topic);

        let mut options = MqttOptions::new(&config.client_id, &config.host, config.port);
        options.set_keep_alive(Duration::from_secs(30));

        if let (Some(username), Some(password)) = (&config.username, &config.password) {
            options.set_credentials(username, password);
        }

        options.set_last_will(rumqttc::LastWill {
            topic: topics.availability.clone(),
            message: "offline".into(),
            qos: QoS::AtLeastOnce,
            retain: true,
        });

        let (client, event_loop) = AsyncClient::new(options, 100);

        info!(
            host = %config.host,
            port = config.port,
            client_id = %config.client_id,
            "MQTT client created"
        );

        Ok((
            Self {
                client,
                topics,
                current_state: TrainState::standby(),
                last_cmd: None,
                last_cmd_time: Instant::now(),
            },
            event_loop,
        ))
    }

    /// Run the MQTT actor event loop.
    pub async fn run(
        mut self,
        mut event_loop: rumqttc::EventLoop,
        cmd_tx: mpsc::Sender<Command>,
        mut status_rx: mpsc::Receiver<StatusUpdate>,
        mut executed_rx: mpsc::Receiver<CommandExecuted>,
    ) -> Result<()> {
        info!("MQTT actor started");

        let mut subscribed = false;
        let mut paused_until: Option<TokioInstant> = None;

        loop {
            tokio::select! {
                biased;

                // Resume event-loop polling after backoff. Status drain stays
                // active throughout so the BLE actor is never blocked on us.
                _ = async {
                    match paused_until {
                        Some(t) => sleep_until(t).await,
                        None => std::future::pending().await,
                    }
                }, if paused_until.is_some() => {
                    paused_until = None;
                }

                event = event_loop.poll(), if paused_until.is_none() => {
                    match event {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            info!("Connected to MQTT broker");
                            if let Err(e) = self.client
                                .publish(&self.topics.availability, QoS::AtLeastOnce, true, "online")
                                .await
                            {
                                warn!(error = %e, "Failed to publish availability; will retry on next ConnAck");
                                continue;
                            }
                            if !subscribed {
                                if let Err(e) = self.subscribe_to_topics().await {
                                    warn!(error = %e, "Subscribe failed; will retry on next ConnAck");
                                    continue;
                                }
                                subscribed = true;
                            }
                            if let Err(e) = self.publish_state().await {
                                warn!(error = %e, "Initial state publish failed");
                            }
                        }
                        Ok(Event::Incoming(Packet::Publish(publish))) => {
                            if let Some(command) = self.route_publish(&publish.topic, &publish.payload) {
                                if let Command::Train(train_cmd) = command {
                                    if self.should_block_duplicate(train_cmd) {
                                        debug!(?train_cmd, "Blocked duplicate command");
                                        continue;
                                    }
                                    self.last_cmd = Some(train_cmd);
                                    self.last_cmd_time = Instant::now();
                                }

                                debug!(?command, "Received command");
                                if cmd_tx.send(command).await.is_err() {
                                    error!("BLE actor channel closed");
                                    break;
                                }
                            }
                        }
                        Ok(Event::Incoming(Packet::SubAck(_))) => {
                            debug!("Subscription acknowledged");
                        }
                        Ok(_) => {}
                        Err(e) => {
                            warn!(error = %e, "MQTT event error; backing off");
                            subscribed = false;
                            paused_until = Some(TokioInstant::now() + MQTT_ERROR_BACKOFF);
                        }
                    }
                }

                Some(update) = status_rx.recv() => {
                    if let Err(e) = self.handle_status_update(update).await {
                        warn!(error = %e, "Failed to publish status update");
                    }
                }

                Some(executed) = executed_rx.recv() => {
                    if let Err(e) = self.publish_executed(executed.cmd).await {
                        warn!(error = %e, "Failed to publish executed event");
                    }
                }
            }
        }

        Ok(())
    }

    /// Subscribe to command topics.
    async fn subscribe_to_topics(&self) -> Result<()> {
        for topic in [
            self.topics.cmd.as_str(),
            self.topics.led_set.as_str(),
            self.topics.sound_set.as_str(),
        ] {
            self.client
                .subscribe(topic, QoS::AtLeastOnce)
                .await
                .with_context(|| format!("Failed to subscribe to {}", topic))?;
            debug!(topic = %topic, "Subscribed");
        }
        Ok(())
    }

    /// Route an incoming MQTT publish to a typed command based on its topic.
    fn route_publish(&self, topic: &str, payload: &[u8]) -> Option<Command> {
        let text = String::from_utf8_lossy(payload);
        let text = text.trim();

        let command = if topic == self.topics.cmd {
            text.parse::<TrainCommand>().ok().map(Command::Train)
        } else if topic == self.topics.led_set {
            text.parse::<LedColor>().ok().map(Command::Led)
        } else if topic == self.topics.sound_set {
            text.parse::<DuploSound>().ok().map(Command::Sound)
        } else {
            return None;
        };

        if command.is_none() {
            warn!(topic = %topic, payload = %text, "Ignoring unparseable command payload");
        }
        command
    }

    /// Check if this command should be blocked as duplicate.
    fn should_block_duplicate(&self, cmd: TrainCommand) -> bool {
        is_blocked_duplicate(cmd, self.last_cmd, self.last_cmd_time.elapsed())
    }
}

/// Decide whether `cmd` should be dropped because it repeats `last_cmd`
/// within the dedup window. Pure function so tests can exercise the rule
/// without constructing a full `MqttActor`.
fn is_blocked_duplicate(
    cmd: TrainCommand,
    last_cmd: Option<TrainCommand>,
    last_elapsed: Duration,
) -> bool {
    if cmd.allows_repeat() {
        return false;
    }
    matches!(last_cmd, Some(last) if last == cmd && last_elapsed < Duration::from_secs(2))
}

impl MqttActor {
    /// Handle status update from BLE actor.
    async fn handle_status_update(&mut self, update: StatusUpdate) -> Result<()> {
        match update {
            StatusUpdate::State(state) => {
                self.current_state.apply(state);
            }
            StatusUpdate::Battery(pct) => {
                self.current_state.battery = Some(pct);
                self.current_state.touch();
            }
            StatusUpdate::Motor(speed) => {
                self.current_state.motor = Some(speed);
                self.current_state.touch();
            }
            StatusUpdate::Speed(speed) => {
                self.current_state.speed = Some(speed);
                self.current_state.touch();
            }
            StatusUpdate::Led(color) => {
                self.current_state.led = Some(color);
                self.current_state.touch();
            }
            StatusUpdate::Sound(sound) => {
                self.current_state.last_sound = Some(sound);
                self.current_state.touch();
            }
            StatusUpdate::ConnectionState(state) => {
                self.current_state.status = state;
                if state == ConnectionState::Standby {
                    self.current_state.motor = None;
                    self.current_state.speed = None;
                }
                self.current_state.touch();
            }
            StatusUpdate::Attempts(attempts) => {
                self.current_state.attempts = attempts;
                self.current_state.touch();
            }
            StatusUpdate::Error(error) => {
                warn!(error = %error, "BLE error received");
                self.current_state.touch();
            }
        }

        self.publish_state().await
    }

    /// Publish current state to MQTT.
    async fn publish_state(&self) -> Result<()> {
        let payload = serde_json::to_vec(&self.current_state)?;
        self.client
            .publish(&self.topics.state, QoS::AtLeastOnce, true, payload)
            .await
            .context("Failed to publish state")?;

        debug!(state = ?self.current_state, "Published state");
        Ok(())
    }

    /// Publish command executed event.
    async fn publish_executed(&self, cmd: TrainCommand) -> Result<()> {
        let payload = serde_json::json!({"cmd": cmd.to_string()});
        self.client
            .publish(
                &self.topics.executed,
                QoS::AtLeastOnce,
                false, // Not retained
                payload.to_string(),
            )
            .await
            .context("Failed to publish executed event")?;

        debug!(?cmd, "Published executed event");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MotorConfig;

    mod mqtt_config {
        use super::*;

        #[test]
        fn default_values() {
            let config = MqttConfig::default();
            assert_eq!(config.host, "localhost");
            assert_eq!(config.port, 1883);
            assert!(config.username.is_none());
            assert!(config.password.is_none());
            assert_eq!(config.client_id, "duplo-train-gateway");
        }

        #[test]
        fn deserialize_partial_config() {
            let vars = vec![("HOST".to_string(), "custom-host".to_string())];
            let config: MqttConfig = serde_env::from_iter(vars).unwrap();
            assert_eq!(config.host, "custom-host");
            assert_eq!(config.port, 1883);
        }

        #[test]
        fn deserialize_full_config() {
            let vars = vec![
                ("HOST".to_string(), "mqtt.example.com".to_string()),
                ("PORT".to_string(), "8883".to_string()),
                ("USERNAME".to_string(), "user".to_string()),
                ("PASSWORD".to_string(), "pass".to_string()),
                ("CLIENT_ID".to_string(), "my-client".to_string()),
            ];
            let config: MqttConfig = serde_env::from_iter(vars).unwrap();

            assert_eq!(config.host, "mqtt.example.com");
            assert_eq!(config.port, 8883);
            assert_eq!(config.username, Some("user".to_string()));
            assert_eq!(config.password, Some("pass".to_string()));
            assert_eq!(config.client_id, "my-client");
        }
    }

    mod motor_config {
        use super::*;

        #[test]
        fn default_values() {
            let config = MotorConfig::default();
            assert_eq!(config.forward, 50);
            assert_eq!(config.boost, 75);
            assert_eq!(config.boost_duration, None);
            assert_eq!(config.backward, -50);
            assert_eq!(config.backward_delay, 1200);
        }

        #[test]
        fn boost_duration_zero_becomes_none() {
            let vars = vec![("MOTOR_BOOST_DURATION".to_string(), "0".to_string())];
            let config: MotorConfig = serde_env::from_iter(vars).unwrap();
            assert_eq!(config.boost_duration, None);
        }

        #[test]
        fn boost_duration_positive_value() {
            let vars = vec![("MOTOR_BOOST_DURATION".to_string(), "10".to_string())];
            let config: MotorConfig = serde_env::from_iter(vars).unwrap();
            assert_eq!(config.boost_duration, Some(10));
        }
    }

    mod topics {
        use super::*;

        #[test]
        fn constructs_correct_paths() {
            let topics = Topics::new("my/topic");
            assert_eq!(topics.cmd, "my/topic/cmd");
            assert_eq!(topics.led_set, "my/topic/led/set");
            assert_eq!(topics.sound_set, "my/topic/sound/set");
            assert_eq!(topics.state, "my/topic/state");
            assert_eq!(topics.executed, "my/topic/executed");
            assert_eq!(topics.availability, "my/topic/availability");
        }
    }

    mod battery_persistence {
        use super::*;

        #[test]
        fn standby_preserves_battery() {
            let mut state = TrainState::connected();
            state.battery = Some(85);
            state.apply(TrainState::standby());
            assert_eq!(state.battery, Some(85));
            assert_eq!(state.status, ConnectionState::Standby);
        }

        #[test]
        fn connected_preserves_battery() {
            let mut state = TrainState::standby();
            state.battery = Some(60);
            state.apply(TrainState::connected());
            assert_eq!(state.battery, Some(60));
            assert_eq!(state.status, ConnectionState::Connected);
        }

        #[test]
        fn no_battery_stays_none() {
            let mut state = TrainState::standby();
            assert!(state.battery.is_none());
            state.apply(TrainState::connected());
            assert!(state.battery.is_none());
        }

        #[test]
        fn battery_update_still_works() {
            let mut state = TrainState::connected();
            state.battery = Some(85);
            state.battery = Some(70);
            assert_eq!(state.battery, Some(70));
        }

        #[test]
        fn led_preserved_across_state_transition() {
            let mut state = TrainState::connected();
            state.led = Some(crate::types::LedColor::Red);
            state.apply(TrainState::standby());
            assert_eq!(state.led, Some(crate::types::LedColor::Red));
        }

        #[test]
        fn last_sound_preserved_across_state_transition() {
            let mut state = TrainState::connected();
            state.last_sound = Some(crate::types::DuploSound::Horn);
            state.apply(TrainState::standby());
            assert_eq!(state.last_sound, Some(crate::types::DuploSound::Horn));
        }
    }

    mod duplicate_blocking_logic {
        use super::*;
        use std::time::Duration;

        #[test]
        fn forward_not_blocked_even_if_repeated() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Forward,
                Some(TrainCommand::Forward),
                Duration::from_millis(100)
            ));
        }

        #[test]
        fn boost_not_blocked_even_if_repeated() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Boost,
                Some(TrainCommand::Boost),
                Duration::from_millis(100)
            ));
        }

        #[test]
        fn backward_blocked_if_repeated_within_2s() {
            assert!(is_blocked_duplicate(
                TrainCommand::Backward,
                Some(TrainCommand::Backward),
                Duration::from_millis(500)
            ));
        }

        #[test]
        fn stop_blocked_if_repeated_within_2s() {
            assert!(is_blocked_duplicate(
                TrainCommand::Stop,
                Some(TrainCommand::Stop),
                Duration::from_millis(500)
            ));
        }

        #[test]
        fn backward_not_blocked_after_2s() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Backward,
                Some(TrainCommand::Backward),
                Duration::from_millis(2100)
            ));
        }

        #[test]
        fn stop_not_blocked_after_2s() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Stop,
                Some(TrainCommand::Stop),
                Duration::from_millis(2100)
            ));
        }

        #[test]
        fn different_command_not_blocked() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Backward,
                Some(TrainCommand::Stop),
                Duration::from_millis(100)
            ));
        }

        #[test]
        fn first_command_not_blocked() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Stop,
                None,
                Duration::from_millis(0)
            ));
        }

        #[test]
        fn backward_after_forward_not_blocked() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Backward,
                Some(TrainCommand::Forward),
                Duration::from_millis(100)
            ));
        }

        #[test]
        fn stop_after_boost_not_blocked() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Stop,
                Some(TrainCommand::Boost),
                Duration::from_millis(100)
            ));
        }

        #[test]
        fn exactly_2s_not_blocked() {
            assert!(!is_blocked_duplicate(
                TrainCommand::Stop,
                Some(TrainCommand::Stop),
                Duration::from_secs(2)
            ));
        }

        #[test]
        fn just_under_2s_blocked() {
            assert!(is_blocked_duplicate(
                TrainCommand::Stop,
                Some(TrainCommand::Stop),
                Duration::from_millis(1999)
            ));
        }
    }
}
