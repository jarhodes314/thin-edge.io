//! An adapter to help the c8y-mapper communicate with older versions of the agent
//! that are still using the `tedge/commands/#` topics.
//!
//! The aim is to be able to handle the case where the mapper has been updated but not the agent.
//! This will typically arises during a self update where the tedge packages are updated by the agent.
//! In that case, tedge-agent is not restarted and the old version is running with the new version of the mapper.
//! The latter must then handle properly the communication with on old version of the agent.
//! Unfortunately, this might not be just a transient state as the user might forget to restart the agent.
//! Hence, the mapper has to support restart, software list and software update operations.
//! - This adapter will have to be removed once the old `tedge/commands/#` topics fully deprecated.
//! - It works only for the main device
//! - It doesn't handle the case where the agent has been updated but not the mapper:
//!   this adaptation can only be done by the agent (in the tedge_to_te converter)
//!   and requires specific care to avoid an infinite loop of tedge <-> conversion.
//!
//! This adapter:
//! - listen to `te/device/main///cmd//+` and `tedge/commands/res/#`
//!   (more specifically only the topics related to restart, software_list and software_update)
//! - republish the commands received on one these topic to the corresponding one
//! - extract command identifiers from the `te` topics and inject these in `tedge` payloads
//! - extract command identifiers from the `tedge` payloads and inject these in the `te` topics
//! - make sure `te` messages are retained and `tedge` messages are not.

use serde_json::Value;
use std::convert::Infallible;
use tedge_actors::Converter;
use tedge_actors::ConvertingActor;
use tedge_actors::ConvertingActorBuilder;
use tedge_actors::MessageSink;
use tedge_actors::MessageSource;
use tedge_actors::NoConfig;
use tedge_config::models::TopicPrefix;
use tedge_mqtt_ext::MqttMessage;
use tedge_mqtt_ext::QoS;
use tedge_mqtt_ext::Topic;
use tedge_mqtt_ext::TopicFilter;
use tracing::log::error;

pub struct OldAgentAdapter {
    prefix: String,
}

impl OldAgentAdapter {
    pub fn builder(
        topic_prefix: &TopicPrefix,
        mqtt: &mut (impl MessageSource<MqttMessage, TopicFilter> + MessageSink<MqttMessage>),
    ) -> ConvertingActorBuilder<OldAgentAdapter> {
        let mut builder = ConvertingActor::builder(
            "OldAgentAdapter",
            OldAgentAdapter {
                prefix: format!("{topic_prefix}-mapper"),
            },
        );
        builder.connect_source(old_and_new_command_topics(), mqtt);
        builder.connect_sink(NoConfig, mqtt);
        builder
    }
}

impl Converter for OldAgentAdapter {
    type Input = MqttMessage;
    type Output = MqttMessage;
    type Error = Infallible;

    fn convert(&mut self, input: &Self::Input) -> Result<Vec<Self::Output>, Self::Error> {
        match try_convert(&self.prefix, input) {
            Ok(Some(output)) => Ok(vec![output]),
            Ok(None) => Ok(vec![]),
            Err(error) => {
                error!("Fail to convert agent command over te <-> tedge topics: {error}");
                Ok(vec![])
            }
        }
    }
}

/// Include old response topics for command as well as new command topics for agent operation
fn old_and_new_command_topics() -> TopicFilter {
    [
        "tedge/commands/res/control/restart",
        "tedge/commands/res/software/list",
        "tedge/commands/res/software/update",
        "te/device/main///cmd/restart/+",
        "te/device/main///cmd/software_list/+",
        "te/device/main///cmd/software_update/+",
    ]
    .into_iter()
    .map(TopicFilter::new_unchecked)
    .collect()
}

fn try_convert(prefix: &str, input: &MqttMessage) -> Result<Option<MqttMessage>, String> {
    let topic = input.topic.name.as_str();
    let payload = input.payload_bytes();
    match topic.split('/').collect::<Vec<&str>>()[..] {
        ["tedge", "commands", "res", "control", "restart"] => {
            convert_from_old_agent_response(prefix, "restart", payload)
        }
        ["tedge", "commands", "res", "software", "list"] => {
            convert_from_old_agent_response(prefix, "software_list", payload)
        }
        ["tedge", "commands", "res", "software", "update"] => {
            convert_from_old_agent_response(prefix, "software_update", payload)
        }
        [_, "device", "main", "", "", "cmd", "restart", cmd_id] => {
            convert_to_old_agent_request("control/restart", cmd_id, payload)
        }
        [_, "device", "main", "", "", "cmd", "software_list", cmd_id] => {
            convert_to_old_agent_request("software/list", cmd_id, payload)
        }
        [_, "device", "main", "", "", "cmd", "software_update", cmd_id] => {
            convert_to_old_agent_request("software/update", cmd_id, payload)
        }
        _ => Ok(None),
    }
}

fn convert_to_old_agent_request(
    cmd_type: &str,
    cmd_id: &str,
    payload: &[u8],
) -> Result<Option<MqttMessage>, String> {
    if payload.is_empty() {
        // Ignore clearing message
        return Ok(None);
    }
    if let Ok(Value::Object(mut request)) = serde_json::from_slice(payload) {
        if let Some(Value::String(status)) = request.get("status") {
            if status != "init" {
                // Ignore non-init state
                return Ok(None);
            }
            request.insert("id".to_string(), Value::String(cmd_id.to_string()));
            request.remove("status"); // as the old agent denies the unknown init status
            if let Ok(updated_payload) = serde_json::to_vec(&Value::Object(request)) {
                if let Ok(topic) = Topic::new(&format!("tedge/commands/req/{cmd_type}")) {
                    return Ok(Some(
                        MqttMessage::new(&topic, updated_payload).with_qos(QoS::AtLeastOnce),
                    ));
                }
            }
        }
    }

    Err(format!(
        "Fail to inject command 'id' into agent {cmd_type} request: {}",
        std::str::from_utf8(payload).unwrap_or("non utf8 payload")
    ))
}

fn convert_from_old_agent_response(
    prefix: &str,
    cmd_type: &str,
    payload: &[u8],
) -> Result<Option<MqttMessage>, String> {
    if let Ok(Value::Object(response)) = serde_json::from_slice(payload) {
        if let Some(Value::String(cmd_id)) = response.get("id") {
            // The new mapper expects command ids with a specific prefix
            let topic_name = if cmd_id.starts_with(prefix) {
                format!("te/device/main///cmd/{cmd_type}/{cmd_id}")
            } else {
                format!("te/device/main///cmd/{cmd_type}/{prefix}-{cmd_id}")
            };
            if let Ok(topic) = Topic::new(&topic_name) {
                return Ok(Some(
                    MqttMessage::new(&topic, payload)
                        .with_retain()
                        .with_qos(QoS::AtLeastOnce),
                ));
            }
        }
    }

    Err(format!(
        "Fail to extract command 'id' from agent {cmd_type} response: {}",
        std::str::from_utf8(payload).unwrap_or("non utf8 payload")
    ))
}
