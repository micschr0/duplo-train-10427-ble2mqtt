//! LEGO DUPLO 10427 BLE-to-MQTT Gateway

#![forbid(unsafe_code)]

mod ble;
mod config;
mod mqtt;
mod protocol;
mod types;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::ble::BleActor;
use crate::config::{MotorConfig, MqttConfig};
use crate::mqtt::MqttActor;
use crate::types::{Command, CommandExecuted, StatusUpdate};

const COMMAND_CHANNEL_SIZE: usize = 32;
const STATUS_CHANNEL_SIZE: usize = 32;
const EXECUTED_CHANNEL_SIZE: usize = 32;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting DUPLO Train Gateway"
    );

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(COMMAND_CHANNEL_SIZE);
    let (status_tx, status_rx) = mpsc::channel::<StatusUpdate>(STATUS_CHANNEL_SIZE);
    let (executed_tx, executed_rx) = mpsc::channel::<CommandExecuted>(EXECUTED_CHANNEL_SIZE);

    let mqtt_config = MqttConfig::from_env()?;
    let motor_config = MotorConfig::from_env()?;

    info!(
        forward = motor_config.forward,
        boost = motor_config.boost,
        boost_duration = ?motor_config.boost_duration,
        backward = motor_config.backward,
        backward_delay = motor_config.backward_delay,
        "Motor configuration loaded"
    );

    let ble_actor = BleActor::new()
        .await
        .context("Failed to initialize BLE actor")?;

    let (mqtt_actor, mqtt_event_loop) = MqttActor::new(mqtt_config)
        .await
        .context("Failed to initialize MQTT actor")?;

    info!("Actors initialized, starting event loops");

    let ble_handle = tokio::spawn(async move {
        if let Err(e) = ble_actor
            .run(cmd_rx, status_tx, executed_tx, motor_config)
            .await
        {
            error!(error = %e, "BLE actor failed");
        }
    });

    let mqtt_handle = tokio::spawn(async move {
        if let Err(e) = mqtt_actor
            .run(mqtt_event_loop, cmd_tx, status_rx, executed_rx)
            .await
        {
            error!(error = %e, "MQTT actor failed");
        }
    });

    // Wait for either actor to complete (which indicates an error)
    tokio::select! {
        result = ble_handle => {
            error!("BLE actor terminated unexpectedly");
            result?;
        }
        result = mqtt_handle => {
            error!("MQTT actor terminated unexpectedly");
            result?;
        }
    }

    Ok(())
}

/// Initialize tracing with environment filter.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,duplo_train_controller=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();
}
