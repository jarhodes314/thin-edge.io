//! Inspects incoming operation requests for URLs to files and downloads them into the File Transfer
//! Service, so that they can be downloaded more quickly by the child devices.
//!
//! Payloads of some operations contain `remoteUrl` property which contains a URL from which we
//! need to download a file. However, child devices may have reduced speed or even may not be able
//! to reach the remote URL at all. Additionally, multiple child devices may require the same file,
//! so it makes sense to download it once and place it inside the File Transfer Service, so that
//! child devices may download the files quickly on the local network.
//!
//! This actor, for all child devices, for operations that have `remoteUrl` property, tries to
//! download the file from this URL, places it in the File Transfer Service, and inserts the URL to
//! download the file from the FTS in the `tedgeUrl` property.

use async_trait::async_trait;
use camino::Utf8PathBuf;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tedge_actors::fan_in_message_type;
use tedge_actors::Actor;
use tedge_actors::Builder;
use tedge_actors::CloneSender;
use tedge_actors::DynSender;
use tedge_actors::LinkError;
use tedge_actors::LoggingReceiver;
use tedge_actors::MessageReceiver;
use tedge_actors::MessageSink;
use tedge_actors::MessageSource;
use tedge_actors::RuntimeError;
use tedge_actors::RuntimeRequest;
use tedge_actors::RuntimeRequestSink;
use tedge_actors::Sender;
use tedge_actors::Service;
use tedge_actors::SimpleMessageBoxBuilder;
use tedge_api::commands::CommandPayload;
use tedge_api::commands::ConfigUpdateCmdPayload;
use tedge_api::mqtt_topics::Channel;
use tedge_api::mqtt_topics::ChannelFilter;
use tedge_api::mqtt_topics::EntityFilter;
use tedge_api::mqtt_topics::EntityTopicId;
use tedge_api::mqtt_topics::MqttSchema;
use tedge_api::mqtt_topics::OperationType;
use tedge_api::path::DataDir;
use tedge_api::CommandStatus;
use tedge_downloader_ext::DownloadRequest;
use tedge_downloader_ext::DownloadResult;
use tedge_mqtt_ext::MqttMessage;
use tedge_mqtt_ext::Topic;
use tedge_mqtt_ext::TopicFilter;
use tracing::error;
use tracing::info;
use tracing::warn;

type IdDownloadRequest = (String, DownloadRequest);
type IdDownloadResult = (String, DownloadResult);

fan_in_message_type!(FileCacheInput[MqttMessage, IdDownloadResult]: Debug);

pub struct FileCacheActor {
    input_receiver: LoggingReceiver<FileCacheInput>,
    mqtt_sender: DynSender<MqttMessage>,
    downloader_sender: DynSender<IdDownloadRequest>,

    tedge_http_host: Arc<str>,
    mqtt_schema: MqttSchema,
    data_dir: DataDir,
    device_topic_id: EntityTopicId,

    pending_operations: HashMap<String, ConfigUpdateCmdPayload>,
}

#[async_trait]
impl Actor for FileCacheActor {
    fn name(&self) -> &str {
        "FileCacheActor"
    }

    async fn run(mut self) -> Result<(), RuntimeError> {
        while let Some(message) = self.input_receiver.recv().await {
            match message {
                FileCacheInput::MqttMessage(message) => self.process_mqtt_message(message).await?,
                FileCacheInput::IdDownloadResult(message) => self.process_download(message).await?,
            }
        }

        Ok(())
    }
}

impl FileCacheActor {
    async fn process_mqtt_message(
        &mut self,
        mqtt_message: MqttMessage,
    ) -> Result<(), RuntimeError> {
        let Ok((entity, channel)) = self.mqtt_schema.entity_channel_of(&mqtt_message.topic) else {
            return Ok(());
        };

        let Channel::Command {
            operation: OperationType::ConfigUpdate,
            cmd_id,
        } = channel
        else {
            return Ok(());
        };

        let update_payload =
            match serde_json::from_slice::<ConfigUpdateCmdPayload>(mqtt_message.payload.as_bytes())
            {
                Ok(payload) => payload,
                Err(err) => {
                    warn!("Received config update, but payload is malformed: {err}");
                    return Ok(());
                }
            };

        // Caching is a best-effort optimization to pre-seed the File Transfer Service for child
        // devices. For the local device the agent downloads directly from `remoteUrl`, so caching
        // is pointless and a failed cache download would spuriously fail the operation.
        if update_payload.remote_url.is_empty()
            || update_payload.tedge_url.is_some()
            || entity == self.device_topic_id
        {
            return Ok(());
        }

        match &update_payload.status {
            CommandStatus::Executing => {
                self.download_config_file_to_cache(&mqtt_message.topic, &update_payload)
                    .await?;
            }
            CommandStatus::Successful => self.delete_symlink_for_config_update(
                &entity,
                &update_payload.config_type,
                &cmd_id,
            )?,
            CommandStatus::Failed { .. } => self.delete_symlink_for_config_update(
                &entity,
                &update_payload.config_type,
                &cmd_id,
            )?,
            _ => {}
        }

        Ok(())
    }

    async fn process_download(&mut self, download: IdDownloadResult) -> Result<(), RuntimeError> {
        let (topic, result) = download;

        let Some(mut operation) = self.pending_operations.remove(&topic) else {
            return Ok(());
        };

        let (entity, Channel::Command { cmd_id, .. }) = self
            .mqtt_schema
            .entity_channel_of(&topic)
            .expect("only topics targeting config update command should be inserted")
        else {
            return Ok(());
        };

        let download = match result {
            // if cant download file, operation failed
            Err(err) => {
                let error_message = format!("tedge-agent failed downloading a file: {err}");
                operation.failed(&error_message);
                error!("{}", error_message);
                let message = MqttMessage::new(
                    &Topic::new_unchecked(&topic),
                    serde_json::to_string(&operation).unwrap(),
                );
                self.mqtt_sender.send(message).await?;
                return Ok(());
            }
            Ok(download) => download,
        };

        self.create_symlink_for_config_update(
            &entity,
            &operation.config_type,
            &cmd_id,
            download.file_path,
        )?;

        let url_symlink_path = symlink_path(&entity, &operation.config_type, &cmd_id);

        let tedge_url = format!(
            "http://{}/te/v1/files/{}",
            &self.tedge_http_host, url_symlink_path
        );

        operation.tedge_url = Some(tedge_url);

        let mqtt_message = MqttMessage::new(
            &Topic::new(&topic).unwrap(),
            serde_json::to_string(&operation).unwrap(),
        );
        self.mqtt_sender.send(mqtt_message).await.unwrap();

        Ok(())
    }

    async fn download_config_file_to_cache(
        &mut self,
        config_update_topic: &Topic,
        config_update_payload: &ConfigUpdateCmdPayload,
    ) -> Result<(), RuntimeError> {
        let remote_url = &config_update_payload.remote_url;

        let file_cache_key = sha256::digest(remote_url);
        let dest_path = self.data_dir.cache_dir().path().join(file_cache_key);
        let topic = config_update_topic.name.clone();

        info!("Downloading config file from `{remote_url}` to cache");

        let download_request = DownloadRequest::new(remote_url, dest_path.as_std_path());

        self.pending_operations.insert(
            config_update_topic.name.clone(),
            config_update_payload.clone(),
        );

        self.downloader_sender
            .send((topic, download_request))
            .await?;

        Ok(())
    }

    fn create_symlink_for_config_update(
        &self,
        entity_topic_id: &EntityTopicId,
        config_type: &str,
        cmd_id: &str,
        original: impl AsRef<Path>,
    ) -> Result<(), RuntimeError> {
        let symlink_path = self.fs_file_transfer_symlink_path(entity_topic_id, config_type, cmd_id);

        if !symlink_path.is_symlink() {
            std::fs::create_dir_all(symlink_path.parent().unwrap())
                .and_then(|_| std::os::unix::fs::symlink(original, &symlink_path))
                .map_err(|e| RuntimeError::ActorError(e.into()))?;
        }

        Ok(())
    }

    fn delete_symlink_for_config_update(
        &self,
        entity_topic_id: &EntityTopicId,
        config_type: &str,
        cmd_id: &str,
    ) -> Result<(), RuntimeError> {
        let symlink_path = self.fs_file_transfer_symlink_path(entity_topic_id, config_type, cmd_id);

        if let Err(e) = std::fs::remove_file(symlink_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                // we're missing permissions or trying to delete a directory
                return Err(RuntimeError::ActorError(e.into()))?;
            }
        }

        Ok(())
    }

    fn fs_file_transfer_symlink_path(
        &self,
        entity_topic_id: &EntityTopicId,
        config_type: &str,
        cmd_id: &str,
    ) -> Utf8PathBuf {
        let symlink_dir_path = self.data_dir.file_transfer_dir();

        symlink_dir_path
            .path()
            .join(symlink_path(entity_topic_id, config_type, cmd_id))
    }
}

fn symlink_path(entity_topic_id: &EntityTopicId, config_type: &str, cmd_id: &str) -> Utf8PathBuf {
    Utf8PathBuf::from(entity_topic_id.as_str().replace('/', "_"))
        .join("config_update")
        .join(format!("{}-{cmd_id}", config_type.replace('/', ":")))
}

pub struct FileCacheActorBuilder {
    message_box: SimpleMessageBoxBuilder<FileCacheInput, MqttMessage>,
    mqtt_sender: DynSender<MqttMessage>,
    download_sender: DynSender<IdDownloadRequest>,

    mqtt_schema: MqttSchema,
    tedge_http_host: Arc<str>,
    data_dir: DataDir,
    device_topic_id: EntityTopicId,
}

impl FileCacheActorBuilder {
    pub fn new(
        mqtt_schema: MqttSchema,
        tedge_http_host: Arc<str>,
        data_dir: DataDir,
        device_topic_id: EntityTopicId,
        downloader_actor: &mut impl Service<IdDownloadRequest, IdDownloadResult>,
        mqtt_actor: &mut (impl MessageSource<MqttMessage, TopicFilter> + MessageSink<MqttMessage>),
    ) -> Self {
        let message_box = SimpleMessageBoxBuilder::new("RestartManager", 10);

        let download_sender =
            downloader_actor.connect_client(message_box.get_sender().sender_clone());

        let mqtt_sender = mqtt_actor.get_sender();
        mqtt_actor.connect_sink(Self::subscriptions(&mqtt_schema), &message_box.get_sender());

        Self {
            message_box,
            mqtt_sender,
            download_sender,
            mqtt_schema,
            tedge_http_host,
            data_dir,
            device_topic_id,
        }
    }

    fn subscriptions(mqtt_schema: &MqttSchema) -> TopicFilter {
        mqtt_schema.topics(
            EntityFilter::AnyEntity,
            ChannelFilter::Command(OperationType::ConfigUpdate),
        )
    }
}

impl MessageSink<FileCacheInput> for FileCacheActorBuilder {
    fn get_sender(&self) -> DynSender<FileCacheInput> {
        self.message_box.get_sender()
    }
}

impl RuntimeRequestSink for FileCacheActorBuilder {
    fn get_signal_sender(&self) -> DynSender<RuntimeRequest> {
        self.message_box.get_signal_sender()
    }
}

impl Builder<FileCacheActor> for FileCacheActorBuilder {
    type Error = LinkError;

    fn try_build(self) -> Result<FileCacheActor, Self::Error> {
        Ok(self.build())
    }

    fn build(self) -> FileCacheActor {
        let (_, rx) = self.message_box.build().into_split();
        FileCacheActor {
            mqtt_sender: self.mqtt_sender,
            downloader_sender: self.download_sender,
            input_receiver: rx,

            tedge_http_host: self.tedge_http_host,
            mqtt_schema: self.mqtt_schema,
            data_dir: self.data_dir,
            device_topic_id: self.device_topic_id,

            pending_operations: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tedge_actors::test_helpers::FakeServerBox;
    use tedge_actors::test_helpers::FakeServerBoxBuilder;
    use tedge_actors::test_helpers::MessageReceiverExt;
    use tedge_actors::test_helpers::TimedMessageBox;
    use tedge_actors::Builder;
    use tedge_actors::MessageReceiver;
    use tedge_actors::Sender;
    use tedge_actors::SimpleMessageBox;
    use tedge_test_utils::fs::TempTedgeDir;
    use tedge_utils::paths::TedgePaths;

    const TEST_TIMEOUT: Duration = Duration::from_millis(500);

    type MqttMessageBox = TimedMessageBox<SimpleMessageBox<MqttMessage, MqttMessage>>;
    type DownloaderMessageBox = TimedMessageBox<FakeServerBox<IdDownloadRequest, IdDownloadResult>>;

    #[tokio::test]
    async fn does_not_pre_cache_config_files_for_the_local_device() {
        let ttd = TempTedgeDir::new();
        let (mut mqtt, mut downloader) = spawn_file_cache_actor(&ttd);

        // A config_update targeting the device the agent itself serves
        let topic = Topic::new_unchecked("te/device/main///cmd/config_update/c8y-1234");
        mqtt.send(executing_config_update(&topic)).await.unwrap();

        // The cache download is pointless for the local device, so none is requested
        assert_eq!(downloader.recv().await, None);
        // and the operation is not marked as failed
        assert_eq!(mqtt.recv().await, None);
    }

    #[tokio::test]
    async fn pre_caches_config_files_for_child_devices() {
        let ttd = TempTedgeDir::new();
        let (mut mqtt, mut downloader) = spawn_file_cache_actor(&ttd);

        // A config_update targeting a child device
        let topic = Topic::new_unchecked("te/device/child01///cmd/config_update/c8y-1234");
        mqtt.send(executing_config_update(&topic)).await.unwrap();

        // The file is downloaded once into the cache for the child device to fetch locally
        let (download_topic, request) = downloader.recv().await.unwrap();
        assert_eq!(download_topic, topic.name);
        assert_eq!(request.url, "http://test.example/file");
    }

    fn executing_config_update(topic: &Topic) -> MqttMessage {
        let payload = r#"{"status":"executing","remoteUrl":"http://test.example/file","serverUrl":"http://test.example","type":"mytype"}"#;
        MqttMessage::new(topic, payload)
    }

    fn spawn_file_cache_actor(ttd: &TempTedgeDir) -> (MqttMessageBox, DownloaderMessageBox) {
        let mut mqtt_builder: SimpleMessageBoxBuilder<MqttMessage, MqttMessage> =
            SimpleMessageBoxBuilder::new("MQTT", 10);
        let mut downloader_builder: FakeServerBoxBuilder<IdDownloadRequest, IdDownloadResult> =
            FakeServerBoxBuilder::default();

        let data_dir = DataDir::from(TedgePaths::from_root_with_defaults(ttd.utf8_path(), "", ""));

        let actor_builder = FileCacheActorBuilder::new(
            MqttSchema::default(),
            "127.0.0.1:8000".into(),
            data_dir,
            EntityTopicId::default_main_device(),
            &mut downloader_builder,
            &mut mqtt_builder,
        );

        let actor = actor_builder.build();
        tokio::spawn(async move { actor.run().await });

        (
            mqtt_builder.build().with_timeout(TEST_TIMEOUT),
            downloader_builder.build().with_timeout(TEST_TIMEOUT),
        )
    }
}
