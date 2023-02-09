use crate::child_device::get_child_id_from_child_topic;
use crate::child_device::FirmwareOperationResponse;
use crate::download::DownloadFirmwareStatusMessage;
use crate::download::FirmwareDownloadManager;
use c8y_api::http_proxy::C8YHttpProxy;
use c8y_api::smartrest::message::collect_smartrest_messages;
use c8y_api::smartrest::smartrest_deserializer::SmartRestFirmwareRequest;
use c8y_api::smartrest::smartrest_deserializer::SmartRestRequestGeneric;
use c8y_api::smartrest::smartrest_serializer::TryIntoOperationStatusMessage;
use c8y_api::smartrest::topic::C8yTopic;
use mqtt_channel::Connection;
use mqtt_channel::Message;
use mqtt_channel::MqttError;
use mqtt_channel::SinkExt;
use mqtt_channel::StreamExt;
use mqtt_channel::Topic;
use mqtt_channel::TopicFilter;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tedge_api::health::health_check_topics;
use tedge_api::health::health_status_down_message;
use tedge_api::health::send_health_status;
use tokio::sync::Mutex;
use tracing::error;
use tracing::info;

pub const PLUGIN_SERVICE_NAME: &str = "c8y-firmware-plugin";
pub const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(10); //TODO: Make this configurable in the first drop
const FIRMWARE_UPDATE_RESPONSE_TOPICS: &str = "tedge/+/commands/res/firmware_update";

#[derive(Debug, Eq, PartialEq, Default, Clone, Deserialize, Hash)]
#[serde(deny_unknown_fields)]
pub struct FirmwareEntry {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
}

impl FirmwareEntry {
    pub fn new(name: &str, version: &str, url: &str, sha256: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            url: url.to_string(),
            sha256: sha256.to_string(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ActiveOperationState {
    Pending,
    Executing,
}

pub struct FirmwareManager {
    mqtt_client: Connection,
    c8y_request_topics: TopicFilter,
    health_check_topics: TopicFilter,
    firmware_update_response_topics: TopicFilter,
    firmware_download_manager: FirmwareDownloadManager,
}

impl FirmwareManager {
    pub async fn new(
        tedge_device_id: impl ToString,
        mqtt_port: u16,
        http_client: Arc<Mutex<dyn C8YHttpProxy>>,
        local_http_host: impl ToString,
        tmp_dir: PathBuf,
        config_dir: PathBuf, // /etc/tedge
    ) -> Result<Self, anyhow::Error> {
        let mqtt_client = Self::create_mqtt_client(mqtt_port).await?;

        let c8y_request_topics = C8yTopic::SmartRestRequest.into();
        let health_check_topics = health_check_topics(PLUGIN_SERVICE_NAME);
        let firmware_update_response_topics =
            TopicFilter::new_unchecked(FIRMWARE_UPDATE_RESPONSE_TOPICS);

        let firmware_download_manager = FirmwareDownloadManager::new(
            tedge_device_id.to_string(),
            mqtt_client.published.clone(),
            http_client.clone(),
            local_http_host.to_string(),
            config_dir.clone(),
            tmp_dir.clone(),
        );

        Ok(FirmwareManager {
            mqtt_client,
            c8y_request_topics,
            health_check_topics,
            firmware_update_response_topics,
            firmware_download_manager,
        })
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        self.get_pending_operations_from_cloud().await?;

        // Now the firmware plugin is done with the initialization and ready for processing the messages
        send_health_status(&mut self.mqtt_client.published, PLUGIN_SERVICE_NAME).await;
        info!("Ready to serve the firmware request.");

        loop {
            tokio::select! {
                message = self.mqtt_client.received.next() => {
                    if let Some(message) = message {
                        let topic = message.topic.name.clone();
                        if let Err(err) = self.process_mqtt_message(
                            message,
                        )
                        .await {
                            error!("Processing the message received on {topic} failed with {err}");
                        }
                    } else {
                        // message is None and the connection has been closed
                        return Ok(())
                    }
                }
                Some(((child_id, hash), op_state)) = self.firmware_download_manager.operation_timer.next_timed_out_entry() => {
                    info!("Child device did not response with the timeout period of 10s. firmware hash: {hash}");
                    self.fail_pending_firmware_operation_in_c8y(child_id.clone(), op_state,
                        format!("Timeout due to lack of response from child device: {child_id} for hash: {hash}")).await?;
                }
                // No inotify
            }
        }
    }

    async fn process_mqtt_message(&mut self, message: Message) -> Result<(), anyhow::Error> {
        if self.health_check_topics.accept(&message) {
            send_health_status(&mut self.mqtt_client.published, PLUGIN_SERVICE_NAME).await;
            return Ok(());
        } else if self.firmware_update_response_topics.accept(&message) {
            self.handle_child_device_firmware_operation_response(&message)
                .await?
        } else if self.c8y_request_topics.accept(&message) {
            for smartrest_message in collect_smartrest_messages(message.payload_str()?) {
                let result = match smartrest_message.split(',').next().unwrap_or_default() {
                    "515" => {
                        if let Ok(firmware_request) =
                            SmartRestFirmwareRequest::from_smartrest(smartrest_message.as_str())
                        {
                            self.firmware_download_manager
                                .handle_firmware_download_request(firmware_request)
                                .await
                        } else {
                            error!("Incorrect SmartREST payload: {smartrest_message}");
                            Ok(())
                        }
                    }
                    _ => {
                        // Ignore operation messages not meant for this plugin
                        Ok(())
                    }
                };

                if let Err(err) = result {
                    error!("Handling of operation: '{smartrest_message}' failed with {err}");
                }
            }
        } else {
            error!(
                "Received unexpected message on topic: {}",
                message.topic.name
            );
        }
        Ok(())
    }

    pub async fn handle_child_device_firmware_operation_response(
        &mut self,
        message: &Message,
    ) -> Result<(), anyhow::Error> {
        match FirmwareOperationResponse::try_from(message) {
            Ok(response) => {
                let smartrest_responses = self
                    .firmware_download_manager
                    .handle_child_device_firmware_update_response(&response)?;

                for smartrest_response in smartrest_responses {
                    self.mqtt_client.published.send(smartrest_response).await?
                }

                Ok(())
            }
            Err(err) => {
                let child_id = get_child_id_from_child_topic(&message.topic.name)?;

                self.fail_pending_firmware_operation_in_c8y(
                    child_id,
                    ActiveOperationState::Pending,
                    err.to_string(),
                )
                .await
            }
        }
    }

    async fn fail_pending_firmware_operation_in_c8y(
        &mut self,
        child_id: String,
        op_state: ActiveOperationState,
        failure_reason: String,
    ) -> Result<(), anyhow::Error> {
        let c8y_child_topic =
            Topic::new_unchecked(&C8yTopic::ChildSmartRestResponse(child_id).to_string());

        let executing_msg = Message::new(
            &c8y_child_topic,
            DownloadFirmwareStatusMessage::status_executing()?,
        );
        let failed_msg = Message::new(
            &c8y_child_topic,
            DownloadFirmwareStatusMessage::status_failed(failure_reason)?,
        );

        if op_state == ActiveOperationState::Pending {
            self.mqtt_client.published.send(executing_msg).await?;
        }

        self.mqtt_client.published.send(failed_msg).await?;

        Ok(())
    }

    async fn create_mqtt_client(mqtt_port: u16) -> Result<Connection, anyhow::Error> {
        let mut topic_filter = TopicFilter::new_unchecked(&C8yTopic::SmartRestRequest.to_string()); //c8y/s/ds
        topic_filter.add_all(health_check_topics(PLUGIN_SERVICE_NAME));
        topic_filter.add_all(TopicFilter::new_unchecked(FIRMWARE_UPDATE_RESPONSE_TOPICS));

        let mqtt_config = mqtt_channel::Config::default()
            .with_session_name(PLUGIN_SERVICE_NAME)
            .with_port(mqtt_port)
            .with_subscriptions(topic_filter)
            .with_last_will_message(health_status_down_message(PLUGIN_SERVICE_NAME));

        let mqtt_client = Connection::new(&mqtt_config).await?;
        Ok(mqtt_client)
    }

    async fn get_pending_operations_from_cloud(&mut self) -> Result<(), MqttError> {
        let msg = Message::new(&C8yTopic::SmartRestResponse.to_topic()?, "500");
        self.mqtt_client.published.send(msg).await?;
        Ok(())
    }
}
