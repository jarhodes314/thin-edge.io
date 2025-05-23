//! This module provides some helper functions to create SmartREST messages
//! that can be used to create various managed objects in Cumulocity inventory.

// TODO: Have different SmartREST messages be different types, so we can see
// where these messages are used, not only created.
//
// TODO: both `C8yTopic::smartrest_response_topic(&EntityMetadata)` and
// `publish_topic_from_ancestors(&[String])` produce C8y MQTT topics on which
// smartrest messages are sent. There should be one comprehensive API for
// generating them.

use crate::smartrest::topic::publish_topic_from_parent;
use crate::smartrest::topic::C8yTopic;
use mqtt_channel::MqttMessage;
use std::time::Duration;
use tedge_config::models::TopicPrefix;

use super::message_ids::CHILD_DEVICE_CREATION;
use super::message_ids::SERVICE_CREATION;
use super::message_ids::SET_CURRENTLY_INSTALLED_CONFIGURATION;
use super::message_ids::SET_DEVICE_PROFILE_THAT_IS_BEING_APPLIED;
use super::message_ids::SET_REQUIRED_AVAILABILITY;
use super::payload::SmartrestPayload;

/// Create a SmartREST message for creating a child device under the given ancestors.
///
/// The provided ancestors list must contain all the parents of the given device
/// starting from its immediate parent device.
pub fn child_device_creation_message(
    child_id: &str,
    device_name: Option<&str>,
    device_type: Option<&str>,
    parent: Option<&str>,
    main_device_id: &str,
    prefix: &TopicPrefix,
    with_device_marker: bool,
) -> Result<MqttMessage, InvalidValueError> {
    if child_id.is_empty() {
        return Err(InvalidValueError {
            field_name: "child_id".to_string(),
            value: child_id.to_string(),
        });
    }
    if let Some("") = device_name {
        return Err(InvalidValueError {
            field_name: "device_name".to_string(),
            value: "".to_string(),
        });
    }
    if let Some("") = device_type {
        return Err(InvalidValueError {
            field_name: "device_type".to_string(),
            value: "".to_string(),
        });
    }

    let payload = SmartrestPayload::serialize((
        CHILD_DEVICE_CREATION,
        child_id,
        device_name.unwrap_or(child_id),
        device_type.unwrap_or("thin-edge.io-child"),
        with_device_marker,
    ))
    .expect("child_id, device_name, device_type should not increase payload size over the limit");

    Ok(MqttMessage::new(
        &publish_topic_from_parent(parent, main_device_id, prefix),
        payload.into_inner(),
    ))
}

/// Create a SmartREST message for creating a service on device.
/// The provided ancestors list must contain all the parents of the given service
/// starting from its immediate parent device.
pub fn service_creation_message(
    service_id: &str,
    service_name: &str,
    service_type: &str,
    service_status: &str,
    parent: Option<&str>,
    main_device_id: &str,
    prefix: &TopicPrefix,
) -> Result<MqttMessage, InvalidValueError> {
    Ok(MqttMessage::new(
        &publish_topic_from_parent(parent, main_device_id, prefix),
        service_creation_message_payload(service_id, service_name, service_type, service_status)?
            .into_inner(),
    ))
}

/// Create a SmartREST message for creating a service on device.
/// The provided ancestors list must contain all the parents of the given service
/// starting from its immediate parent device.
pub fn service_creation_message_payload(
    service_id: &str,
    service_name: &str,
    service_type: &str,
    service_status: &str,
) -> Result<SmartrestPayload, InvalidValueError> {
    // TODO: most of this noise can be eliminated by implementing `Serialize`/`Deserialize` for smartrest format
    if service_id.is_empty() {
        return Err(InvalidValueError {
            field_name: "service_id".to_string(),
            value: service_id.to_string(),
        });
    }
    if service_name.is_empty() {
        return Err(InvalidValueError {
            field_name: "service_name".to_string(),
            value: service_name.to_string(),
        });
    }
    if service_type.is_empty() {
        return Err(InvalidValueError {
            field_name: "service_type".to_string(),
            value: service_type.to_string(),
        });
    }
    if service_status.is_empty() {
        return Err(InvalidValueError {
            field_name: "service_status".to_string(),
            value: service_status.to_string(),
        });
    }

    let payload = SmartrestPayload::serialize((
        SERVICE_CREATION,
        service_id,
        service_type,
        service_name,
        service_status,
    ))
    .expect(
        "TODO: message can get over the limit but none of the fields can be reasonably trimmed",
    );

    Ok(payload)
}

pub fn set_c8y_config_fragment(
    config_type: &str,
    remote_url: &str,
    name: Option<&str>,
) -> SmartrestPayload {
    SmartrestPayload::serialize((
        SET_CURRENTLY_INSTALLED_CONFIGURATION,
        config_type,
        remote_url,
        name,
    ))
    .expect("shouldn't put payload over size limit")
}

/// Create a SmartREST message to set a response interval for c8y_RequiredAvailability.
///
/// In the SmartREST 117 message, the interval must be in MINUTES, and can be <=0,
/// which means the device is in maintenance mode in the c8y context.
/// Details: https://cumulocity.com/docs/device-integration/fragment-library/#device-availability
#[derive(Debug)]
pub struct C8ySmartRestSetInterval117 {
    pub c8y_topic: C8yTopic,
    pub interval: Duration,
    pub prefix: TopicPrefix,
}

impl From<C8ySmartRestSetInterval117> for MqttMessage {
    fn from(value: C8ySmartRestSetInterval117) -> Self {
        let topic = value.c8y_topic.to_topic(&value.prefix).unwrap();
        let interval_in_minutes = value.interval.as_secs() / 60;
        let payload =
            SmartrestPayload::serialize((SET_REQUIRED_AVAILABILITY, &interval_in_minutes))
                .expect("interval should not increase size over the limit");
        MqttMessage::new(&topic, payload.into_inner())
    }
}

/// Create a SmartREST payload for setting/updating the current state of the
/// target profile in its own managed object.
///
/// When all individual operations are finished (i.e. firmware update, software
/// update and configuration update), the `profile_executed` field should be set
/// to `true`, otherwise it should be `false`.
pub fn set_c8y_profile_target_payload(profile_executed: bool) -> SmartrestPayload {
    SmartrestPayload::serialize((SET_DEVICE_PROFILE_THAT_IS_BEING_APPLIED, profile_executed))
        .expect("shouldn't put payload over size limit")
}

#[derive(thiserror::Error, Debug)]
#[error("Field `{field_name}` contains invalid value: {value:?}")]
pub struct InvalidValueError {
    field_name: String,
    value: String,
}
