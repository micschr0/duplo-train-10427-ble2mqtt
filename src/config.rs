//! Configuration loaded from environment variables.

use anyhow::{Context, Result};
use serde::Deserialize;

/// MQTT configuration from environment variables.
/// Prefix: `MQTT_`
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client_id: String,
    pub base_topic: String,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 1883,
            username: None,
            password: None,
            client_id: "duplo-train-gateway".to_string(),
            base_topic: "duplo/train".to_string(),
        }
    }
}

impl MqttConfig {
    /// Load configuration from environment variables with `MQTT_` prefix.
    pub fn from_env() -> Result<Self> {
        serde_env::from_env_with_prefix("MQTT")
            .context("Failed to parse MQTT configuration from environment")
    }
}

/// Motor speed configuration from environment variables.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MotorConfig {
    #[serde(rename = "motor_forward")]
    pub forward: i8,
    #[serde(rename = "motor_boost")]
    pub boost: i8,
    /// Optional boost duration in seconds. If set, boost automatically
    /// reverts to forward speed after this duration. 0 or unset = unlimited.
    #[serde(rename = "motor_boost_duration", default)]
    pub boost_duration: Option<u64>,
    #[serde(rename = "motor_backward")]
    pub backward: i8,
    #[serde(rename = "backward_delay")]
    pub backward_delay: u64,
}
impl Default for MotorConfig {
    fn default() -> Self {
        Self {
            forward: 50,
            boost: 75,
            boost_duration: None,
            backward: -50,
            backward_delay: 1200,
        }
    }
}

impl MotorConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let mut cfg: Self = serde_env::from_env()
            .context("Failed to parse motor configuration from environment")?;
        cfg.normalize();
        cfg.validate()?;
        Ok(cfg)
    }

    /// Normalize deserialized values. Call after any deserialization.
    /// Maps `boost_duration: Some(0)` to `None` (0 means unlimited).
    pub(crate) fn normalize(&mut self) {
        if self.boost_duration == Some(0) {
            self.boost_duration = None;
        }
    }

    /// Verify motor speeds are within the protocol's valid signed range.
    fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.forward) {
            anyhow::bail!("MOTOR_FORWARD must be in 1..=100 (got {})", self.forward);
        }
        if !(1..=100).contains(&self.boost) {
            anyhow::bail!("MOTOR_BOOST must be in 1..=100 (got {})", self.boost);
        }
        if !(-100..=-1).contains(&self.backward) {
            anyhow::bail!(
                "MOTOR_BACKWARD must be in -100..=-1 (got {})",
                self.backward
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(forward: i8, boost: i8, backward: i8) -> MotorConfig {
        MotorConfig {
            forward,
            boost,
            boost_duration: None,
            backward,
            backward_delay: 1200,
        }
    }

    #[test]
    fn validate_accepts_defaults() {
        assert!(MotorConfig::default().validate().is_ok());
    }

    #[test]
    fn validate_accepts_boundaries() {
        assert!(cfg(1, 1, -1).validate().is_ok());
        assert!(cfg(100, 100, -100).validate().is_ok());
    }

    #[test]
    fn validate_rejects_forward_zero() {
        let err = cfg(0, 75, -50).validate().unwrap_err();
        assert!(err.to_string().contains("MOTOR_FORWARD"));
    }

    #[test]
    fn validate_rejects_forward_negative() {
        assert!(cfg(-10, 75, -50).validate().is_err());
    }

    #[test]
    fn validate_rejects_forward_over_100() {
        assert!(cfg(101, 75, -50).validate().is_err());
    }

    #[test]
    fn validate_rejects_boost_zero() {
        let err = cfg(50, 0, -50).validate().unwrap_err();
        assert!(err.to_string().contains("MOTOR_BOOST"));
    }

    #[test]
    fn validate_rejects_boost_over_100() {
        assert!(cfg(50, 101, -50).validate().is_err());
    }

    #[test]
    fn validate_rejects_backward_zero() {
        let err = cfg(50, 75, 0).validate().unwrap_err();
        assert!(err.to_string().contains("MOTOR_BACKWARD"));
    }

    #[test]
    fn validate_rejects_backward_positive() {
        assert!(cfg(50, 75, 10).validate().is_err());
    }

    #[test]
    fn validate_rejects_backward_below_neg100() {
        assert!(cfg(50, 75, -101).validate().is_err());
    }
}
