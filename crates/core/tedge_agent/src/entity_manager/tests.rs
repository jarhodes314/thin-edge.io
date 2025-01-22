use crate::entity_manager::server::EntityStoreResponse;
use proptest::proptest;
use std::collections::HashSet;
use tedge_actors::Server;
use tedge_api::entity::EntityMetadata;

#[tokio::test]
async fn new_entity_store() {
    let (mut entity_store, _mqtt_output) = entity::server("device-under-test");

    assert_eq!(
        entity::get(&mut entity_store, "device/main//").await,
        Some(EntityMetadata::main_device("device-under-test".to_string()))
    )
}

proptest! {
    #[test]
    fn it_works_for_any_registration_order(registrations in model::walk(6)) {
        tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (mut entity_store, _mqtt_output) = entity::server("device-under-test");
            let mut state = model::State::new();

            for (protocol,action) in registrations {
                let expected_updates = state.apply(protocol, action.clone());
                let actual_updates = match entity_store.handle((protocol,action).into()).await {
                    EntityStoreResponse::Create(Ok(registered_entities)) => {
                        registered_entities
                            .iter()
                            .map(|registered_entity| registered_entity.reg_message.topic_id.clone())
                            .collect()
                    },
                    EntityStoreResponse::Delete(actual_updates) => HashSet::from_iter(actual_updates),
                    _ => HashSet::new(),
                };
                assert_eq!(actual_updates, expected_updates);
            }
        })
    }
}

mod entity {
    use crate::entity_manager::server::EntityStoreRequest;
    use crate::entity_manager::server::EntityStoreResponse;
    use crate::entity_manager::server::EntityStoreServer;
    use std::str::FromStr;
    use tedge_actors::Builder;
    use tedge_actors::NoMessage;
    use tedge_actors::Server;
    use tedge_actors::SimpleMessageBox;
    use tedge_actors::SimpleMessageBoxBuilder;
    use tedge_api::entity::EntityMetadata;
    use tedge_api::entity_store::EntityRegistrationMessage;
    use tedge_api::mqtt_topics::EntityTopicId;
    use tedge_api::mqtt_topics::MqttSchema;
    use tedge_api::EntityStore;
    use tedge_mqtt_ext::MqttMessage;
    use tempfile::TempDir;

    pub async fn get(
        entity_store: &mut EntityStoreServer,
        topic_id: &str,
    ) -> Option<EntityMetadata> {
        let topic_id = EntityTopicId::from_str(topic_id).unwrap();
        if let EntityStoreResponse::Get(entity) =
            entity_store.handle(EntityStoreRequest::Get(topic_id)).await
        {
            return entity;
        };
        None
    }

    pub fn server(
        device_id: &str,
    ) -> (EntityStoreServer, SimpleMessageBox<MqttMessage, NoMessage>) {
        let mqtt_schema = MqttSchema::default();
        let main_device = EntityRegistrationMessage::main_device(Some(device_id.to_string()));
        let default_service_type = "default_service_type".to_string();
        let telemetry_cache_size = 0;
        let log_dir = TempDir::new().unwrap();
        let clean_start = true;
        let entity_auto_register = true;
        let entity_store = EntityStore::with_main_device_and_default_service_type(
            mqtt_schema.clone(),
            main_device,
            default_service_type,
            telemetry_cache_size,
            log_dir,
            clean_start,
        )
        .unwrap();

        let mut mqtt_actor = SimpleMessageBoxBuilder::new("MQTT", 64);
        let server = EntityStoreServer::new(
            entity_store,
            mqtt_schema,
            &mut mqtt_actor,
            entity_auto_register,
        );

        let mqtt_output = mqtt_actor.build();
        (server, mqtt_output)
    }
}

mod model {
    use crate::entity_manager::server::EntityStoreRequest;
    use proptest::prelude::*;
    use std::collections::HashMap;
    use std::collections::HashSet;
    use tedge_api::entity::EntityType;
    use tedge_api::entity_store::EntityRegistrationMessage;
    use tedge_api::mqtt_topics::EntityTopicId;
    use tedge_api::mqtt_topics::MqttSchema;

    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    #[allow(clippy::upper_case_acronyms)]
    pub enum Protocol {
        HTTP,
        MQTT,
    }

    #[derive(Debug, Clone)]
    pub enum Action {
        AddDevice {
            topic: String,
            props: Vec<(String, String)>,
        },
        AddService {
            topic: String,
            props: Vec<(String, String)>,
        },
        RemDevice {
            topic: String,
        },
        RemService {
            topic: String,
        },
    }

    impl Action {
        pub fn target(&self) -> &str {
            match self {
                Action::AddDevice { topic, .. }
                | Action::AddService { topic, .. }
                | Action::RemDevice { topic }
                | Action::RemService { topic } => topic.as_ref(),
            }
        }

        fn parent(&self) -> Option<(&str, &str)> {
            match self {
                Action::AddDevice { topic, .. }
                | Action::AddService { topic, .. }
                | Action::RemDevice { topic }
                | Action::RemService { topic } => {
                    let len = topic.len();
                    let topic = topic.as_str();
                    match len {
                        0 => None,
                        1 => Some(("main", &topic[0..1])),
                        _ => Some((&topic[0..(len - 1)], &topic[(len - 1)..len])),
                    }
                }
            }
        }

        pub fn topic_id(&self) -> EntityTopicId {
            match (self.parent(), &self) {
                (None, _) => EntityTopicId::default_main_device(),
                (Some(_), Action::AddDevice { topic, .. })
                | (Some(_), Action::RemDevice { topic }) => {
                    format!("device/{topic}//").parse().unwrap()
                }
                (Some((parent, id)), Action::AddService { .. })
                | (Some((parent, id)), Action::RemService { .. }) => {
                    format!("device/{parent}/service/{id}").parse().unwrap()
                }
            }
        }

        pub fn parent_topic_id(&self) -> Option<EntityTopicId> {
            self.parent()
                .map(|(parent, _)| format!("device/{parent}//").parse().unwrap())
        }

        pub fn target_type(&self) -> EntityType {
            match (self.parent(), &self) {
                (None, _) => EntityType::MainDevice,

                (Some(_), Action::AddDevice { .. }) | (Some(_), Action::RemDevice { .. }) => {
                    EntityType::ChildDevice
                }

                (Some(_), Action::AddService { .. }) | (Some(_), Action::RemService { .. }) => {
                    EntityType::Service
                }
            }
        }

        pub fn properties(&self) -> serde_json::Map<String, serde_json::Value> {
            match self {
                Action::AddDevice { props, .. } | Action::AddService { props, .. } => props
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect(),

                Action::RemDevice { .. } | Action::RemService { .. } => serde_json::Map::new(),
            }
        }
    }

    impl From<Action> for EntityRegistrationMessage {
        fn from(action: Action) -> Self {
            EntityRegistrationMessage {
                topic_id: action.topic_id(),
                external_id: None,
                r#type: action.target_type(),
                parent: action.parent_topic_id(),
                other: action.properties(),
            }
        }
    }

    impl From<Action> for EntityStoreRequest {
        fn from(action: Action) -> Self {
            match &action {
                Action::AddDevice { .. } | Action::AddService { .. } => {
                    let registration = EntityRegistrationMessage::from(action);
                    EntityStoreRequest::Create(registration)
                }

                Action::RemDevice { .. } | Action::RemService { .. } => {
                    EntityStoreRequest::Delete(action.topic_id())
                }
            }
        }
    }

    impl From<(Protocol, Action)> for EntityStoreRequest {
        fn from((protocol, action): (Protocol, Action)) -> Self {
            match protocol {
                Protocol::HTTP => EntityStoreRequest::from(action),
                Protocol::MQTT => {
                    let registration = EntityRegistrationMessage::from(action);
                    let message = registration.to_mqtt_message(&MqttSchema::default());
                    EntityStoreRequest::MqttMessage(message)
                }
            }
        }
    }

    type PropMap = serde_json::Map<String, serde_json::Value>;

    pub struct State {
        entities: HashMap<EntityTopicId, (EntityType, Option<EntityTopicId>, PropMap)>,
        registered: HashSet<EntityTopicId>,
    }

    impl State {
        pub fn new() -> Self {
            let mut state = State {
                entities: HashMap::default(),
                registered: HashSet::default(),
            };
            state.apply(
                Protocol::HTTP,
                Action::AddDevice {
                    topic: "".to_string(),
                    props: vec![],
                },
            );
            state
        }

        pub fn apply(&mut self, protocol: Protocol, action: Action) -> HashSet<EntityTopicId> {
            let topic = action.topic_id();

            match action {
                Action::AddDevice { .. } | Action::AddService { .. } => {
                    let parent = action.parent_topic_id();

                    if let Some(parent) = parent.as_ref() {
                        if protocol == Protocol::HTTP && !self.registered.contains(parent) {
                            // Under HTTP, registering a child before its parent is an error
                            return HashSet::new();
                        }
                    }

                    if self.entities.contains_key(&topic) {
                        HashSet::new()
                    } else {
                        let entity_type = action.target_type();
                        self.entities.insert(
                            topic.clone(),
                            (entity_type, parent.clone(), action.properties()),
                        );

                        let new_entities = self.register(topic, parent);
                        if protocol == Protocol::HTTP {
                            new_entities
                        } else {
                            // Under MQTT, no response is sent back
                            HashSet::new()
                        }
                    }
                }

                Action::RemDevice { .. } | Action::RemService { .. } => {
                    if self.entities.contains_key(&topic) {
                        self.entities.remove(&topic);

                        let old_entities = self.deregister(topic);
                        if protocol == Protocol::HTTP {
                            old_entities
                        } else {
                            // Under MQTT, no response is sent back
                            HashSet::new()
                        }
                    } else {
                        HashSet::new()
                    }
                }
            }
        }

        fn register(
            &mut self,
            new_entity: EntityTopicId,
            parent: Option<EntityTopicId>,
        ) -> HashSet<EntityTopicId> {
            if parent
                .as_ref()
                .map_or(true, |p| self.registered.contains(p))
            {
                self.registered.insert(new_entity.clone());
                let new_entities = HashSet::from([new_entity]);
                self.cascade_registration(new_entities)
            } else {
                HashSet::new()
            }
        }

        fn deregister(&mut self, old_entity: EntityTopicId) -> HashSet<EntityTopicId> {
            if self.registered.contains(&old_entity) {
                self.registered.remove(&old_entity);
                let old_entity = HashSet::from([old_entity]);
                self.cascade_deregistration(old_entity)
            } else {
                HashSet::new()
            }
        }

        fn cascade_registration(
            &mut self,
            mut new_entities: HashSet<EntityTopicId>,
        ) -> HashSet<EntityTopicId> {
            let mut new_connected = HashSet::new();
            for (entity_id, (_, parent, _)) in self.entities.iter() {
                if let Some(parent_id) = parent {
                    if !self.registered.contains(entity_id) && new_entities.contains(parent_id) {
                        new_connected.insert(entity_id.clone());
                    }
                }
            }

            if !new_connected.is_empty() {
                for entity_id in &new_connected {
                    self.registered.insert(entity_id.clone());
                }

                for entity_id in self.cascade_registration(new_connected) {
                    new_entities.insert(entity_id);
                }
            }

            new_entities
        }

        fn cascade_deregistration(
            &mut self,
            mut old_entities: HashSet<EntityTopicId>,
        ) -> HashSet<EntityTopicId> {
            let mut new_disconnected = HashSet::new();
            for (entity_id, (_, parent, _)) in self.entities.iter() {
                if let Some(parent_id) = parent {
                    if old_entities.contains(parent_id) {
                        new_disconnected.insert(entity_id.clone());
                    }
                }
            }

            if !new_disconnected.is_empty() {
                for entity_id in &new_disconnected {
                    self.entities.remove(entity_id);
                    self.registered.remove(entity_id);
                }

                for entity_id in self.cascade_deregistration(new_disconnected) {
                    old_entities.insert(entity_id);
                }
            }

            old_entities
        }
    }

    prop_compose! {
        pub fn random_protocol()(protocol in "[hm]") -> Protocol {
            if protocol == "h" {
                Protocol::HTTP
            } else {
                Protocol::MQTT
            }
        }
    }

    prop_compose! {
        pub fn random_name()(id in "[abc]{1,3}") -> String {
            id.to_string()
        }
    }

    prop_compose! {
        pub fn random_key()(id in "[xyz]") -> String {
            id.to_string()
        }
    }

    prop_compose! {
        pub fn random_value()(id in "[0-9]") -> String {
            id.to_string()
        }
    }

    prop_compose! {
        pub fn random_prop()(
            key in random_key(),
            value in random_value()
        ) -> (String,String) {
            (key, value)
        }
    }

    prop_compose! {
        pub fn random_props(max_length: usize)(
            vec in prop::collection::vec(random_prop(),
            0..max_length)
        ) -> Vec<(String,String)>
        {
            vec
        }
    }

    prop_compose! {
        pub fn pick_random_or_new(names: Vec<String>)(
            id in 0..(names.len()+1),
            name in random_name()
        ) -> String {
            names.get(id).map(|n| n.to_owned()).unwrap_or(name)
        }
    }

    prop_compose! {
        pub fn random_action_on(topic: String)(
            protocol in random_protocol(),
            action in 1..5,
            props in random_props(2)
        ) -> (Protocol,Action) {
            let topic = topic.to_owned();
            let action = match action {
                1 => Action::AddDevice{ topic, props },
                2 => Action::AddService{ topic, props },
                3 => Action::RemService{ topic },
                _ => Action::RemDevice{ topic },
            };
            (protocol, action)
        }
    }

    pub fn random_action() -> impl Strategy<Value = (Protocol, Action)> {
        random_name().prop_flat_map(random_action_on)
    }

    fn step(actions: Vec<(Protocol, Action)>) -> impl Strategy<Value = Vec<(Protocol, Action)>> {
        let nodes = actions.iter().map(|(_, a)| a.target().to_owned()).collect();
        pick_random_or_new(nodes)
            .prop_flat_map(random_action_on)
            .prop_flat_map(move |action| {
                let mut actions = actions.clone();
                actions.push(action);
                Just(actions)
            })
    }

    pub fn walk(max_length: u32) -> impl Strategy<Value = Vec<(Protocol, Action)>> {
        if max_length == 0 {
            Just(vec![]).boxed()
        } else if max_length == 1 {
            prop::collection::vec(random_action(), 0..=1).boxed()
        } else {
            walk(max_length - 1).prop_flat_map(step).boxed()
        }
    }
}
