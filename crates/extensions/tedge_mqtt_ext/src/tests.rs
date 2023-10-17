use crate::*;
use mqtt_channel::Topic;
use std::time::Duration;
use tedge_actors::Builder;
use tedge_actors::ServiceConsumer;
use tedge_actors::SimpleMessageBox;
use tedge_actors::SimpleMessageBoxBuilder;

type MqttClient = SimpleMessageBox<MqttMessage, MqttMessage>;

struct MqttClientBuilder {
    subscriptions: TopicFilter,
    box_builder: SimpleMessageBoxBuilder<MqttMessage, MqttMessage>,
}

impl MqttClientBuilder {
    fn new(name: &str, subscriptions: &(impl Clone + Into<TopicFilter>)) -> Self {
        MqttClientBuilder {
            subscriptions: subscriptions.clone().into(),
            box_builder: SimpleMessageBoxBuilder::new(name, 16),
        }
    }

    fn new_with_capacity(
        name: &str,
        subscriptions: &(impl Clone + Into<TopicFilter>),
        capacity: usize,
    ) -> Self {
        MqttClientBuilder {
            subscriptions: subscriptions.clone().into(),
            box_builder: SimpleMessageBoxBuilder::new(name, capacity),
        }
    }
}

impl ServiceConsumer<MqttMessage, MqttMessage, TopicFilter> for MqttClientBuilder {
    fn get_config(&self) -> TopicFilter {
        self.subscriptions.clone()
    }

    fn set_request_sender(&mut self, request_sender: DynSender<MqttMessage>) {
        self.box_builder.set_request_sender(request_sender)
    }

    fn get_response_sender(&self) -> DynSender<MqttMessage> {
        self.box_builder.get_response_sender()
    }
}

impl Builder<MqttClient> for MqttClientBuilder {
    type Error = Infallible;

    fn try_build(self) -> Result<MqttClient, Self::Error> {
        Ok(self.build())
    }

    fn build(self) -> MqttClient {
        self.box_builder.build()
    }
}

#[tokio::test]
async fn mqtt_actor_can_reliably_forward_messages_under_load() {
    let broker = mqtt_tests::test_mqtt_broker();
    let mqtt_config = MqttConfig::default().with_port(broker.port);
    let mut mqtt = MqttActorBuilder::new(mqtt_config);

    let mut child_device = MqttClientBuilder::new("Child device", &TopicFilter::empty())
        .with_connection(&mut mqtt)
        .build();

    let c8y_mapper_topic = Topic::new_unchecked("some/tedge/topic");

    // Simulate a mapper with no message buffer, which will make the problem
    // occur more quickly if it exists
    let mut c8y_mapper = MqttClientBuilder::new_with_capacity("C8y mapper", &c8y_mapper_topic, 0)
        .with_connection(&mut mqtt)
        .build();

    // Assume Cumulocity just accepts all messages, so don't bother attaching a client
    let c8y_topic = Topic::new_unchecked("c8y/s/us");

    // Simulate the c8y mapper, forwarding messages from the child device to Cumulocity
    tokio::spawn(async move {
        while let Some(mut msg) = c8y_mapper.recv().await {
            tokio::time::sleep(Duration::from_secs(5)).await;
            msg.topic = c8y_topic.clone();
            // If the actor doesn't process incoming/outgoing MQTT messages concurrently,
            // this will cause a deadlock
            c8y_mapper.send(msg).await.unwrap();
        }
    });

    tokio::spawn(mqtt_actor(mqtt));

    for _ in 1..100 {
        // This timeout should only be triggered if the actor isn't progressing
        tokio::time::timeout(
            Duration::from_millis(50),
            child_device.send(MqttMessage::new(&c8y_mapper_topic, "Hi Bob")),
        )
            .await
            .expect("messages should be forwarded (is the MQTT actor processing incoming/outgoing messages concurrently?)")
            .expect("send should succeed");
    }
}

#[tokio::test]
async fn communicate_over_mqtt() {
    let broker = mqtt_tests::test_mqtt_broker();
    let mqtt_config = MqttConfig::default().with_port(broker.port);
    let mut mqtt = MqttActorBuilder::new(mqtt_config);

    let alice_topic = Topic::new_unchecked("messages/for/alice");
    let mut alice: MqttClient = MqttClientBuilder::new("Alice", &alice_topic)
        .with_connection(&mut mqtt)
        .build();

    let bob_topic = Topic::new_unchecked("messages/for/bob");
    let mut bob: MqttClient = MqttClientBuilder::new("Bob", &bob_topic)
        .with_connection(&mut mqtt)
        .build();

    let mut all_topics = TopicFilter::empty();
    all_topics.add_all(alice_topic.clone().into());
    all_topics.add_all(bob_topic.clone().into());
    let mut spy: MqttClient = MqttClientBuilder::new("Spy", &all_topics)
        .with_connection(&mut mqtt)
        .build();

    tokio::spawn(mqtt_actor(mqtt));

    // Message pub on some topic is received by all subscribers
    assert!(alice
        .send(MqttMessage::new(&bob_topic, "Hi Bob"))
        .await
        .is_ok());
    assert_eq!(
        bob.recv().await,
        Some(MqttMessage::new(&bob_topic, "Hi Bob"))
    );
    assert_eq!(
        spy.recv().await,
        Some(MqttMessage::new(&bob_topic, "Hi Bob"))
    );

    // Messages from misc clients can be published without awaiting for a response
    assert!(bob.send(MqttMessage::new(&alice_topic, "1")).await.is_ok());
    assert!(bob.send(MqttMessage::new(&alice_topic, "2")).await.is_ok());
    assert!(alice.send(MqttMessage::new(&bob_topic, "A")).await.is_ok());
    assert!(bob.send(MqttMessage::new(&alice_topic, "3")).await.is_ok());
    assert!(alice.send(MqttMessage::new(&bob_topic, "B")).await.is_ok());
    assert!(alice.send(MqttMessage::new(&bob_topic, "C")).await.is_ok());

    // A subscriber receives only the messages for its subscriptions
    assert_eq!(bob.recv().await, Some(MqttMessage::new(&bob_topic, "A")));
    assert_eq!(bob.recv().await, Some(MqttMessage::new(&bob_topic, "B")));
    assert_eq!(bob.recv().await, Some(MqttMessage::new(&bob_topic, "C")));

    // When messages are pub/sub on a single topic; they are received in order
    assert_eq!(
        alice.recv().await,
        Some(MqttMessage::new(&alice_topic, "1"))
    );
    assert_eq!(
        alice.recv().await,
        Some(MqttMessage::new(&alice_topic, "2"))
    );
    assert_eq!(
        alice.recv().await,
        Some(MqttMessage::new(&alice_topic, "3"))
    );

    // However when messages are sent by different actors or over several topics
    // Message order can be altered.
    let mut messages = vec![];
    for _i in 0..6 {
        let message = spy.recv().await.expect("some message");
        messages.push(message.payload_str().expect("utf8").to_string());
    }
    messages.sort();
    assert_eq!(messages, vec!["1", "2", "3", "A", "B", "C"])
}

async fn mqtt_actor(builder: MqttActorBuilder) {
    let mqtt_actor = builder.build();
    mqtt_actor.run().await.unwrap()
}
