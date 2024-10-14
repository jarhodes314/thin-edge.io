//! Creating and updating `mosquitto.conf` files for MQTT bridges to different clouds.

mod common_mosquitto_config;
mod config;

pub mod aws;
pub mod azure;
pub mod c8y;

pub use common_mosquitto_config::*;
pub use config::BridgeConfig;
pub use config::BridgeLocation;

pub const TEDGE_BRIDGE_CONF_DIR_PATH: &str = "mosquitto-conf";
