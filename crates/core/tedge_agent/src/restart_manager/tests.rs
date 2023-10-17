use crate::restart_manager::builder::RestartManagerBuilder;
use crate::restart_manager::config::RestartManagerConfig;
use std::time::Duration;
use tedge_actors::test_helpers::MessageReceiverExt;
use tedge_actors::test_helpers::TimedMessageBox;
use tedge_actors::Actor;
use tedge_actors::Builder;
use tedge_actors::DynError;
use tedge_actors::MessageReceiver;
use tedge_actors::Sender;
use tedge_actors::ServiceConsumer;
use tedge_actors::SimpleMessageBox;
use tedge_actors::SimpleMessageBoxBuilder;
use tedge_api::messages::CommandStatus;
use tedge_api::messages::RestartCommandPayload;
use tedge_api::mqtt_topics::EntityTopicId;
use tedge_api::RestartCommand;
use tedge_test_utils::fs::TempTedgeDir;

const TEST_TIMEOUT_MS: Duration = Duration::from_millis(5000);

#[tokio::test]
async fn test_pending_restart_operation() -> Result<(), DynError> {
    let temp_dir = TempTedgeDir::new();
    let content = "operation_id = \'1234\'\noperation = \"Restarting\"";
    temp_dir
        .dir(".agent")
        .file("restart-current-operation")
        .with_raw_content(content);

    let mut converter_box = spawn_restart_manager(&temp_dir).await?;

    converter_box
        .assert_received([RestartCommand {
            target: EntityTopicId::default_main_device(),
            cmd_id: "1234".to_string(),
            payload: RestartCommandPayload {
                status: CommandStatus::Successful,
            },
        }])
        .await;

    Ok(())
}

#[tokio::test]
async fn test_pending_restart_operation_failed() -> Result<(), DynError> {
    let temp_dir = TempTedgeDir::new();
    let content = "operation_id = \'1234\'\noperation = \"Pending\"";
    temp_dir
        .dir(".agent")
        .file("restart-current-operation")
        .with_raw_content(content);

    let mut converter_box = spawn_restart_manager(&temp_dir).await?;

    converter_box
        .assert_received([RestartCommand {
            target: EntityTopicId::default_main_device(),
            cmd_id: "1234".to_string(),
            payload: RestartCommandPayload {
                status: CommandStatus::Failed {
                    reason: "The agent has been restarted but not the device".to_string(),
                },
            },
        }])
        .await;

    Ok(())
}

#[tokio::test]
async fn test_pending_restart_operation_successful() -> Result<(), DynError> {
    let temp_dir = TempTedgeDir::new();
    let content = "operation_id = \'1234\'\noperation = \"Restarting\"";
    temp_dir
        .dir(".agent")
        .file("restart-current-operation")
        .with_raw_content(content);

    let mut converter_box = spawn_restart_manager(&temp_dir).await?;

    converter_box
        .assert_received([RestartCommand {
            target: EntityTopicId::default_main_device(),
            cmd_id: "1234".to_string(),
            payload: RestartCommandPayload {
                status: CommandStatus::Successful,
            },
        }])
        .await;

    Ok(())
}

#[tokio::test]
async fn test_new_restart_operation() -> Result<(), DynError> {
    let temp_dir = TempTedgeDir::new();
    temp_dir.dir(".agent").file("restart-current-operation");

    // Spawn restart manager
    let mut converter_box = spawn_restart_manager(&temp_dir).await?;

    // Simulate RestartOperationRequest
    converter_box
        .send(RestartCommand {
            target: EntityTopicId::default_main_device(),
            cmd_id: "1234".to_string(),
            payload: RestartCommandPayload {
                status: CommandStatus::Init,
            },
        })
        .await?;

    let status = converter_box.recv().await.unwrap().status();
    assert_eq!(status, CommandStatus::Executing);

    // Check the agent restart temp file is created
    assert!(temp_dir.path().join("tedge_agent_restart").exists());

    Ok(())
}

async fn spawn_restart_manager(
    tmp_dir: &TempTedgeDir,
) -> Result<TimedMessageBox<SimpleMessageBox<RestartCommand, RestartCommand>>, DynError> {
    let mut converter_builder: SimpleMessageBoxBuilder<RestartCommand, RestartCommand> =
        SimpleMessageBoxBuilder::new("Converter", 5);

    let config = RestartManagerConfig::new(
        &EntityTopicId::default_main_device(),
        &tmp_dir.utf8_path_buf(),
        &tmp_dir.utf8_path_buf(),
    );

    let mut restart_actor_builder = RestartManagerBuilder::new(config);
    converter_builder.set_connection(&mut restart_actor_builder);

    let converter_box = converter_builder.build().with_timeout(TEST_TIMEOUT_MS);

    let restart_actor = restart_actor_builder.build();
    tokio::spawn(async move { restart_actor.run().await });

    Ok(converter_box)
}
