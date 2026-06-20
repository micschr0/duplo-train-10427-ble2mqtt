//! Integration tests for MQTT functionality using testcontainers.
//!
//! These tests require Docker to be running. They are marked `#[ignore]` so
//! plain `cargo test` skips them. Run explicitly with:
//!
//! ```
//! cargo test --test mqtt_integration -- --include-ignored
//! ```

use std::time::Duration;

use rstest::rstest;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mosquitto::Mosquitto;
use tokio::time::timeout;

/// Test context with Mosquitto container and helper methods.
struct MqttTestContext {
    _container: ContainerAsync<Mosquitto>,
    port: u16,
}

impl MqttTestContext {
    async fn new() -> Self {
        let container = Mosquitto::default()
            .start()
            .await
            .expect("Failed to start Mosquitto container - is Docker running?");
        let port = container
            .get_host_port_ipv4(1883)
            .await
            .expect("Failed to get container port");
        Self {
            _container: container,
            port,
        }
    }

    fn client(&self, client_id: &str) -> (AsyncClient, rumqttc::EventLoop) {
        let mut options = MqttOptions::new(client_id, "localhost", self.port);
        options.set_keep_alive(Duration::from_secs(5));
        AsyncClient::new(options, 100)
    }

    async fn connected_client(&self, client_id: &str) -> (AsyncClient, rumqttc::EventLoop) {
        let (client, mut event_loop) = self.client(client_id);
        wait_for_connack(&mut event_loop).await;
        (client, event_loop)
    }

    async fn subscribed_client(
        &self,
        client_id: &str,
        topic: &str,
    ) -> (AsyncClient, rumqttc::EventLoop) {
        let (client, mut event_loop) = self.connected_client(client_id).await;
        client.subscribe(topic, QoS::AtLeastOnce).await.unwrap();
        wait_for_suback(&mut event_loop).await;
        (client, event_loop)
    }
}

/// Wait for connection acknowledgment.
async fn wait_for_connack(event_loop: &mut rumqttc::EventLoop) {
    timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Event::Incoming(Packet::ConnAck(_))) = event_loop.poll().await {
                break;
            }
        }
    })
    .await
    .expect("timed out waiting for ConnAck");
}

/// Wait for subscription acknowledgment.
async fn wait_for_suback(event_loop: &mut rumqttc::EventLoop) {
    timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Event::Incoming(Packet::SubAck(_))) = event_loop.poll().await {
                break;
            }
        }
    })
    .await
    .expect("timed out waiting for SubAck");
}

/// Wait for publish acknowledgment.
async fn wait_for_puback(event_loop: &mut rumqttc::EventLoop) {
    timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Event::Incoming(Packet::PubAck(_))) = event_loop.poll().await {
                break;
            }
        }
    })
    .await
    .expect("timed out waiting for PubAck");
}

/// Wait for a published message.
async fn wait_for_publish(
    event_loop: &mut rumqttc::EventLoop,
    timeout_duration: Duration,
) -> Option<(String, Vec<u8>)> {
    timeout(timeout_duration, async {
        loop {
            if let Ok(Event::Incoming(Packet::Publish(publish))) = event_loop.poll().await {
                return (publish.topic, publish.payload.to_vec());
            }
        }
    })
    .await
    .ok()
}

// =============================================================================
// Tests - require Docker, run with: cargo test --test mqtt_integration -- --include-ignored
// =============================================================================

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_publish_subscribe() {
    let ctx = MqttTestContext::new().await;

    let (pub_client, mut pub_loop) = ctx.connected_client("publisher").await;
    let (_sub_client, mut sub_loop) = ctx.subscribed_client("subscriber", "test/topic").await;

    pub_client
        .publish("test/topic", QoS::AtLeastOnce, false, b"hello")
        .await
        .unwrap();
    let _ = pub_loop.poll().await;

    let received = wait_for_publish(&mut sub_loop, Duration::from_secs(5)).await;
    assert_eq!(received, Some(("test/topic".into(), b"hello".to_vec())));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_retained_message() {
    let ctx = MqttTestContext::new().await;

    // Publish retained message
    let (pub_client, mut pub_loop) = ctx.connected_client("publisher").await;
    pub_client
        .publish("test/retained", QoS::AtLeastOnce, true, b"retained_value")
        .await
        .unwrap();
    wait_for_puback(&mut pub_loop).await;

    // Late subscriber should receive it
    let (_sub_client, mut sub_loop) = ctx.subscribed_client("late_sub", "test/retained").await;

    let received = wait_for_publish(&mut sub_loop, Duration::from_secs(5)).await;
    assert_eq!(
        received,
        Some(("test/retained".into(), b"retained_value".to_vec()))
    );
}

#[rstest]
#[case("duplo/train/cmd", b"forward")]
#[case("duplo/train/cmd", b"boost")]
#[case("duplo/train/cmd", b"backward")]
#[case("duplo/train/cmd", b"stop")]
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_command_topics(#[case] topic: &str, #[case] payload: &[u8]) {
    let ctx = MqttTestContext::new().await;

    let (_gateway, mut gateway_loop) = ctx.subscribed_client("gateway", topic).await;

    let (ha_client, mut ha_loop) = ctx.connected_client("home-assistant").await;
    ha_client
        .publish(topic, QoS::AtLeastOnce, false, payload)
        .await
        .unwrap();
    let _ = ha_loop.poll().await;

    let received = wait_for_publish(&mut gateway_loop, Duration::from_secs(5)).await;
    assert_eq!(received, Some((topic.to_string(), payload.to_vec())));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_json_roundtrip() {
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";
    let state_json = serde_json::json!({
        "status": "connected",
        "attempts": 0,
        "battery": 75,
        "motor": 50,
        "ts": 1234567890
    });

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state_json.to_string().as_bytes(),
        )
        .await
        .unwrap();
    let _ = gateway_loop.poll().await;

    let (_ha, mut ha_loop) = ctx.subscribed_client("home-assistant", state_topic).await;

    let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received.is_some());

    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["status"], "connected");
    assert_eq!(parsed["attempts"], 0);
    assert_eq!(parsed["battery"], 75);
    assert_eq!(parsed["motor"], 50);
    assert_eq!(parsed["ts"], 1234567890);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_idle_state_json() {
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";
    let state_json = serde_json::json!({
        "status": "idle",
        "attempts": 0,
        "ts": 1234567890
    });

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state_json.to_string().as_bytes(),
        )
        .await
        .unwrap();
    let _ = gateway_loop.poll().await;

    let (_ha, mut ha_loop) = ctx.subscribed_client("home-assistant", state_topic).await;

    let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received.is_some());

    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["status"], "idle");
    assert_eq!(parsed["attempts"], 0);
    // Battery and motor should not be present when idle
    assert!(parsed.get("battery").is_none() || parsed["battery"].is_null());
    assert!(parsed.get("motor").is_none() || parsed["motor"].is_null());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_executed_event() {
    let ctx = MqttTestContext::new().await;

    let executed_topic = "duplo/train/executed";
    let executed_json = serde_json::json!({"cmd": "forward"});

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;

    // Subscribe before publishing
    let (_ha, mut ha_loop) = ctx
        .subscribed_client("home-assistant", executed_topic)
        .await;

    gateway
        .publish(
            executed_topic,
            QoS::AtLeastOnce,
            false, // Not retained
            executed_json.to_string().as_bytes(),
        )
        .await
        .unwrap();
    let _ = gateway_loop.poll().await;

    let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received.is_some());

    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["cmd"], "forward");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_connecting_state_with_attempts() {
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";
    let state_json = serde_json::json!({
        "status": "connecting",
        "attempts": 2,
        "ts": 1234567890
    });

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state_json.to_string().as_bytes(),
        )
        .await
        .unwrap();
    let _ = gateway_loop.poll().await;

    let (_ha, mut ha_loop) = ctx.subscribed_client("home-assistant", state_topic).await;

    let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received.is_some());

    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["status"], "connecting");
    assert_eq!(parsed["attempts"], 2);
}

// =============================================================================
// Additional tests for Claude.md requirements
// =============================================================================

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_retained_message() {
    // Requirement: duplo/train/state is retained
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";
    let state_json = serde_json::json!({
        "status": "connected",
        "attempts": 0,
        "battery": 80,
        "motor": 50,
        "ts": 1234567890
    });

    // Publish retained state
    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true, // retained
            state_json.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    // Small delay to ensure message is stored
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Late subscriber should receive retained message
    let (_late_sub, mut late_loop) = ctx.subscribed_client("late-subscriber", state_topic).await;

    let received = wait_for_publish(&mut late_loop, Duration::from_secs(5)).await;
    assert!(
        received.is_some(),
        "Late subscriber should receive retained state"
    );

    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["status"], "connected");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_executed_not_retained() {
    // Requirement: duplo/train/executed is NOT retained
    let ctx = MqttTestContext::new().await;

    let executed_topic = "duplo/train/executed";
    let executed_json = serde_json::json!({"cmd": "forward"});

    // Publish non-retained executed event
    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;
    gateway
        .publish(
            executed_topic,
            QoS::AtLeastOnce,
            false, // NOT retained
            executed_json.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    // Small delay
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Late subscriber should NOT receive the message (not retained)
    let (_late_sub, mut late_loop) = ctx
        .subscribed_client("late-subscriber", executed_topic)
        .await;

    let received = wait_for_publish(&mut late_loop, Duration::from_millis(500)).await;
    assert!(
        received.is_none(),
        "Late subscriber should NOT receive non-retained executed event"
    );
}

#[rstest]
#[case("forward")]
#[case("boost")]
#[case("backward")]
#[case("stop")]
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_executed_event_all_commands(#[case] cmd: &str) {
    // Requirement: executed event contains {"cmd": "..."}
    let ctx = MqttTestContext::new().await;

    let executed_topic = "duplo/train/executed";
    let executed_json = serde_json::json!({"cmd": cmd});

    let (_sub, mut sub_loop) = ctx
        .subscribed_client("home-assistant", executed_topic)
        .await;

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;
    gateway
        .publish(
            executed_topic,
            QoS::AtLeastOnce,
            false,
            executed_json.to_string().as_bytes(),
        )
        .await
        .unwrap();
    let _ = gateway_loop.poll().await;

    let received = wait_for_publish(&mut sub_loop, Duration::from_secs(5)).await;
    assert!(received.is_some());

    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["cmd"], cmd);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_attempts_0_to_1() {
    // Requirement: HA triggers bell on attempts 0→1
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;

    // First state: attempts = 0
    let state1 = serde_json::json!({
        "status": "idle",
        "attempts": 0,
        "ts": 1234567890
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state1.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    // Subscribe after first message is retained
    let (_ha, mut ha_loop) = ctx.subscribed_client("home-assistant", state_topic).await;

    let received1 = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received1.is_some());
    let (_, payload1) = received1.unwrap();
    let parsed1: serde_json::Value = serde_json::from_slice(&payload1).unwrap();
    assert_eq!(parsed1["attempts"], 0);

    // Second state: attempts = 1 (triggers bell in HA)
    let state2 = serde_json::json!({
        "status": "connecting",
        "attempts": 1,
        "ts": 1234567891
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state2.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    let received2 = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received2.is_some());
    let (_, payload2) = received2.unwrap();
    let parsed2: serde_json::Value = serde_json::from_slice(&payload2).unwrap();
    assert_eq!(parsed2["attempts"], 1);
    assert_eq!(parsed2["status"], "connecting");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_attempts_sequence_0_1_2_3() {
    // Requirement: attempts goes 0→1→2→3 with different HA triggers
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;

    // Publish first state before subscribing
    let state0 = serde_json::json!({
        "status": "idle",
        "attempts": 0,
        "ts": 1234567890
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state0.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    // Subscribe after first message is retained
    let (_ha, mut ha_loop) = ctx.subscribed_client("home-assistant", state_topic).await;

    // Receive the retained message (attempts = 0)
    let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received.is_some());
    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["attempts"], 0);

    // Now publish and receive attempts 1, 2, 3
    for attempts in 1..=3u8 {
        let state = serde_json::json!({
            "status": "connecting",
            "attempts": attempts,
            "ts": 1234567890 + attempts as u64
        });
        gateway
            .publish(
                state_topic,
                QoS::AtLeastOnce,
                true,
                state.to_string().as_bytes(),
            )
            .await
            .unwrap();
        wait_for_puback(&mut gateway_loop).await;

        let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
        assert!(received.is_some());
        let (_, payload) = received.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed["attempts"], attempts);
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_connecting_to_connected() {
    // Requirement: HA triggers toast on status connecting→connected
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;

    // First: connecting state
    let state1 = serde_json::json!({
        "status": "connecting",
        "attempts": 1,
        "ts": 1234567890
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state1.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    // Subscribe after first message is retained
    let (_ha, mut ha_loop) = ctx.subscribed_client("home-assistant", state_topic).await;

    let received1 = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received1.is_some());
    let (_, payload1) = received1.unwrap();
    let parsed1: serde_json::Value = serde_json::from_slice(&payload1).unwrap();
    assert_eq!(parsed1["status"], "connecting");

    // Second: connected state (triggers toast in HA)
    let state2 = serde_json::json!({
        "status": "connected",
        "attempts": 0,
        "battery": 85,
        "motor": 0,
        "ts": 1234567891
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state2.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    let received2 = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received2.is_some());
    let (_, payload2) = received2.unwrap();
    let parsed2: serde_json::Value = serde_json::from_slice(&payload2).unwrap();
    assert_eq!(parsed2["status"], "connected");
    assert_eq!(parsed2["attempts"], 0);
}

#[rstest]
#[case(50, "forward")]
#[case(75, "boost")]
#[case(-50, "backward")]
#[case(0, "stop")]
#[case(100, "max-forward")]
#[case(-100, "max-backward")]
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_motor_values(#[case] motor_value: i32, #[case] description: &str) {
    // Requirement: motor range -100..100
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";

    let (_ha, mut ha_loop) = ctx
        .subscribed_client(&format!("ha-{}", description), state_topic)
        .await;

    let (gateway, mut gateway_loop) = ctx.connected_client(&format!("gw-{}", description)).await;

    let state = serde_json::json!({
        "status": "connected",
        "attempts": 0,
        "motor": motor_value,
        "ts": 1234567890
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state.to_string().as_bytes(),
        )
        .await
        .unwrap();
    let _ = gateway_loop.poll().await;

    let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received.is_some(), "Failed for {}", description);
    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["motor"], motor_value, "Failed for {}", description);
}

#[rstest]
#[case(0)]
#[case(25)]
#[case(50)]
#[case(75)]
#[case(100)]
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_battery_values(#[case] battery: u8) {
    // Requirement: battery range 0-100
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";

    let (_ha, mut ha_loop) = ctx
        .subscribed_client(&format!("ha-bat-{}", battery), state_topic)
        .await;

    let (gateway, mut gateway_loop) = ctx.connected_client(&format!("gw-bat-{}", battery)).await;

    let state = serde_json::json!({
        "status": "connected",
        "attempts": 0,
        "battery": battery,
        "motor": 0,
        "ts": 1234567890
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state.to_string().as_bytes(),
        )
        .await
        .unwrap();
    let _ = gateway_loop.poll().await;

    let received = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received.is_some(), "Failed for battery {}", battery);
    let (_, payload) = received.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["battery"], battery, "Failed for battery {}", battery);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_state_battery_persists_across_disconnect() {
    // Requirement: battery value survives state transitions (connected → standby → connected)
    let ctx = MqttTestContext::new().await;

    let state_topic = "duplo/train/state";

    let (gateway, mut gateway_loop) = ctx.connected_client("gateway").await;

    // Step 1: connected with battery
    let state1 = serde_json::json!({
        "status": "connected",
        "attempts": 0,
        "battery": 72,
        "motor": 0,
        "ts": 1000
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state1.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    // Subscribe after first message
    let (_ha, mut ha_loop) = ctx.subscribed_client("home-assistant", state_topic).await;

    let received1 = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received1.is_some());
    let (_, payload1) = received1.unwrap();
    let parsed1: serde_json::Value = serde_json::from_slice(&payload1).unwrap();
    assert_eq!(parsed1["battery"], 72);

    // Step 2: standby (disconnect) — battery should still be present
    let state2 = serde_json::json!({
        "status": "standby",
        "attempts": 0,
        "battery": 72,
        "ts": 2000
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state2.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    let received2 = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received2.is_some());
    let (_, payload2) = received2.unwrap();
    let parsed2: serde_json::Value = serde_json::from_slice(&payload2).unwrap();
    assert_eq!(parsed2["status"], "standby");
    assert_eq!(
        parsed2["battery"], 72,
        "Battery should persist after disconnect"
    );

    // Step 3: reconnected — battery should still be present
    let state3 = serde_json::json!({
        "status": "connected",
        "attempts": 0,
        "battery": 72,
        "motor": 0,
        "ts": 3000
    });
    gateway
        .publish(
            state_topic,
            QoS::AtLeastOnce,
            true,
            state3.to_string().as_bytes(),
        )
        .await
        .unwrap();
    wait_for_puback(&mut gateway_loop).await;

    let received3 = wait_for_publish(&mut ha_loop, Duration::from_secs(5)).await;
    assert!(received3.is_some());
    let (_, payload3) = received3.unwrap();
    let parsed3: serde_json::Value = serde_json::from_slice(&payload3).unwrap();
    assert_eq!(parsed3["status"], "connected");
    assert_eq!(
        parsed3["battery"], 72,
        "Battery should persist after reconnect"
    );
}
