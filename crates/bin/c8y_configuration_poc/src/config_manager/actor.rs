use crate::file_system_ext::{FileEvent, FileRequest};
use crate::mqtt_ext::MqttMessage;
use async_trait::async_trait;
use tedge_actors::{
    adapt, fan_in_message_type, mpsc, Actor, ChannelError, DynSender, MessageBox, StreamExt,
};
use tedge_http_ext::{HttpError, HttpRequest, HttpResponse};

type HttpResult = Result<HttpResponse, HttpError>;

fan_in_message_type!(ConfigInputAndResponse[MqttMessage, FileEvent, HttpResult] : Debug);
fan_in_message_type!(ConfigInput[MqttMessage, FileEvent] : Debug);
fan_in_message_type!(ConfigOutput[MqttMessage, HttpRequest, FileRequest] : Debug);

pub struct ConfigManagerActor {}

impl ConfigManagerActor {
    pub async fn process_file_event(
        &mut self,
        _event: FileEvent,
        _messages: &mut ConfigManagerMessageBox,
    ) -> Result<(), ChannelError> {
        todo!()
    }

    pub async fn process_mqtt_message(
        &mut self,
        _message: MqttMessage,
        messages: &mut ConfigManagerMessageBox,
    ) -> Result<(), ChannelError> {
        // ..
        let request = HttpRequest::new(Default::default(), "https://my-tenant.c8y.io")
            .expect("well formed url");
        let _response = messages.send_http_request(request).await?;
        // ..
        Ok(())
    }
}

#[async_trait]
impl Actor for ConfigManagerActor {
    type MessageBox = ConfigManagerMessageBox;

    async fn run(mut self, mut messages: Self::MessageBox) -> Result<(), ChannelError> {
        while let Some(event) = messages.events.next().await {
            match event {
                ConfigInput::MqttMessage(message) => {
                    self.process_mqtt_message(message, &mut messages).await?;
                }
                ConfigInput::FileEvent(event) => {
                    self.process_file_event(event, &mut messages).await?;
                }
            }
        }
        Ok(())
    }
}

pub struct ConfigManagerMessageBox {
    pub events: mpsc::Receiver<ConfigInput>,
    pub http_responses: mpsc::Receiver<HttpResult>,
    pub file_watcher: DynSender<FileRequest>,
    pub http_con: DynSender<HttpRequest>,
    pub mqtt_con: DynSender<MqttMessage>,
}

impl ConfigManagerMessageBox {
    pub fn new(
        events: mpsc::Receiver<ConfigInput>,
        http_responses: mpsc::Receiver<HttpResult>,
        file_watcher: DynSender<FileRequest>,
        http_con: DynSender<HttpRequest>,
        mqtt_con: DynSender<MqttMessage>,
    ) -> ConfigManagerMessageBox {
        ConfigManagerMessageBox {
            events,
            http_responses,
            file_watcher,
            http_con,
            mqtt_con,
        }
    }

    async fn send_http_request(
        &mut self,
        request: HttpRequest,
    ) -> Result<HttpResult, ChannelError> {
        self.http_con.send(request).await?;
        if let Some(response) = self.http_responses.next().await {
            Ok(response)
        } else {
            Err(ChannelError::ReceiveError())
        }
    }
}

#[async_trait]
impl MessageBox for ConfigManagerMessageBox {
    type Input = ConfigInputAndResponse;
    type Output = ConfigOutput;

    async fn recv(&mut self) -> Option<Self::Input> {
        tokio::select! {
            Some(message) = self.events.next() => {
                match message {
                    ConfigInput::MqttMessage(message) => {
                        Some(ConfigInputAndResponse::MqttMessage(message))
                    },
                    ConfigInput::FileEvent(message) => {
                        Some(ConfigInputAndResponse::FileEvent(message))
                    }
                }
            },
            Some(message) = self.http_responses.next() => {
                Some(ConfigInputAndResponse::HttpResult(message))
            },
            else => None,
        }
    }

    async fn send(&mut self, message: Self::Output) -> Result<(), ChannelError> {
        match message {
            ConfigOutput::MqttMessage(msg) => self.mqtt_con.send(msg).await,
            ConfigOutput::HttpRequest(msg) => self.http_con.send(msg).await,
            ConfigOutput::FileRequest(msg) => self.file_watcher.send(msg).await,
        }
    }

    fn new_box(capacity: usize, output: DynSender<Self::Output>) -> (DynSender<Self::Input>, Self) {
        let (events_sender, events_receiver) = mpsc::channel(capacity);
        let (http_responses_sender, http_responses_receiver) = mpsc::channel(1);
        let input_sender = FanOutSender {
            events_sender,
            http_responses_sender,
        };
        let message_box = ConfigManagerMessageBox {
            events: events_receiver,
            http_responses: http_responses_receiver,
            file_watcher: adapt(&output.clone()),
            http_con: adapt(&output.clone()),
            mqtt_con: adapt(&output.clone()),
        };
        (input_sender.into(), message_box)
    }
}

// One should be able to have a macro to generate this fan-out sender type
#[derive(Clone)]
struct FanOutSender {
    events_sender: mpsc::Sender<ConfigInput>,
    http_responses_sender: mpsc::Sender<HttpResult>,
}

#[async_trait]
impl tedge_actors::Sender<ConfigInputAndResponse> for FanOutSender {
    async fn send(&mut self, message: ConfigInputAndResponse) -> Result<(), ChannelError> {
        match message {
            ConfigInputAndResponse::MqttMessage(msg) => self.events_sender.send(msg).await,
            ConfigInputAndResponse::FileEvent(msg) => self.events_sender.send(msg).await,
            ConfigInputAndResponse::HttpResult(msg) => self.http_responses_sender.send(msg).await,
        }
    }

    fn sender_clone(&self) -> DynSender<ConfigInputAndResponse> {
        Box::new(self.clone())
    }
}

impl From<FanOutSender> for tedge_actors::DynSender<ConfigInputAndResponse> {
    fn from(sender: FanOutSender) -> Self {
        Box::new(sender)
    }
}
