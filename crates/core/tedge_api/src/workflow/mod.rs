pub mod error;
pub(crate) mod log;
mod on_disk;
pub mod script;
pub mod state;
pub mod supervisor;
mod toml_config;

use crate::mqtt_topics::EntityTopicId;
use crate::mqtt_topics::MqttSchema;
use crate::mqtt_topics::OperationType;
pub use error::*;
use mqtt_channel::MqttMessage;
use mqtt_channel::QoS;
pub use script::*;
use serde::Deserialize;
pub use state::*;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
pub use supervisor::*;

pub type OperationName = String;
pub type StateName = String;
pub type CommandId = String;

/// An OperationWorkflow defines the state machine that rules an operation
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "toml_config::TomlOperationWorkflow")]
pub struct OperationWorkflow {
    /// The operation to which this workflow applies
    pub operation: OperationType,

    /// Mark this workflow as built_in
    pub built_in: bool,

    /// Default action outcome handlers
    pub handlers: DefaultHandlers,

    /// The states of the state machine
    pub states: HashMap<StateName, OperationAction>,
}

/// What needs to be done to advance an operation request in some state
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "toml_config::TomlOperationState")]
pub enum OperationAction {
    /// Nothing has to be done: simply move to the next step.
    /// Such steps are intended to be overridden.
    ///
    /// ```toml
    /// action = "proceed"
    /// on_success = "<state>"
    /// ```
    MoveTo(StateName),

    /// The built-in behavior is used
    ///
    /// ```toml
    /// action = "builtin"
    /// on_success = "<state>"
    /// ```
    BuiltIn,

    /// Await agent restart
    ///
    /// In practice, this command simply waits till a timeout.
    /// If the timeout triggers, this step fails.
    /// If the agent stops before the timeout and finds on restart a persisted state of `await-agent-restart`,
    /// then this step is successful.
    ///
    /// ```toml
    /// action = "await-agent-restart"
    /// on_success = "<state>"
    /// on_error = "<state>"
    /// ```
    AwaitingAgentRestart(AwaitHandlers),

    /// A script has to be executed
    Script(ShellScript, ExitHandlers),

    /// Executes a script but move to the next state without waiting for that script to return
    ///
    /// Notably such a script can trigger a device reboot or an agent restart.
    /// ```toml
    /// background_script = "sudo systemctl restart tedge-agent"
    /// on_exec = "<state>"
    /// ```
    BgScript(ShellScript, BgExitHandlers),

    /// Trigger an operation and move to the next state from where the outcome of the operation will be awaited
    ///
    /// ```toml
    /// operation = "sub_operation"
    /// input_script = "/path/to/sub_operation/input_scrip.sh ${.payload.x}" ${.payload.y}"
    /// input.logfile = "${.payload.logfile}"
    /// on_exec = "awaiting_sub_operation"
    /// ```
    Operation(
        OperationName,
        Option<ShellScript>,
        StateExcerpt,
        BgExitHandlers,
    ),

    /// Await the completion of a sub-operation
    ///
    /// The sub-operation is stored in the command state.
    ///
    /// ```toml
    /// action = "await-operation-completion"
    /// on_success = "<state>"
    /// on_error = "<state>"
    /// ```
    AwaitOperationCompletion(AwaitHandlers, StateExcerpt),

    /// The command has been fully processed and needs to be cleared
    Clear,
}

impl Display for OperationAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            OperationAction::MoveTo(step) => format!("move to {step} state"),
            OperationAction::BuiltIn => "builtin".to_string(),
            OperationAction::AwaitingAgentRestart { .. } => "await agent restart".to_string(),
            OperationAction::Script(script, _) => script.to_string(),
            OperationAction::BgScript(script, _) => script.to_string(),
            OperationAction::Operation(operation, maybe_script, _, _) => match maybe_script {
                None => format!("execute {operation} as sub-operation"),
                Some(script) => format!(
                    "execute {operation} as sub-operation, with input payload derived from: {}",
                    script
                ),
            },
            OperationAction::AwaitOperationCompletion { .. } => {
                "await sub-operation completion".to_string()
            }
            OperationAction::Clear => "wait for the requester to finalize the command".to_string(),
        };
        f.write_str(&str)
    }
}

impl OperationWorkflow {
    /// Return a new OperationWorkflow unless there are errors
    /// such as missing or ill-defined states.
    pub fn try_new(
        operation: OperationType,
        handlers: DefaultHandlers,
        mut states: HashMap<StateName, OperationAction>,
    ) -> Result<Self, WorkflowDefinitionError> {
        // The init state is required
        if !states.contains_key("init") {
            return Err(WorkflowDefinitionError::MissingState {
                state: "init".to_string(),
            });
        }

        // The successful state can be omitted,
        // but must be associated to a `clear` if provided.
        let action_on_success = states
            .entry("successful".to_string())
            .or_insert(OperationAction::Clear);
        if action_on_success != &OperationAction::Clear {
            return Err(WorkflowDefinitionError::InvalidAction {
                state: "successful".to_string(),
                action: format!("{action_on_success:?}"),
            });
        }

        // The failed state can be omitted,
        // but must be associated to a `clear` if provided.
        let action_on_error = states
            .entry("failed".to_string())
            .or_insert(OperationAction::Clear);
        if action_on_error != &OperationAction::Clear {
            return Err(WorkflowDefinitionError::InvalidAction {
                state: "failed".to_string(),
                action: format!("{action_on_error:?}"),
            });
        }

        Ok(OperationWorkflow {
            operation,
            built_in: false,
            handlers,
            states,
        })
    }

    /// Create a built-in operation workflow
    pub fn built_in(operation: OperationType) -> Self {
        let states = [
            ("init", OperationAction::MoveTo("scheduled".to_string())),
            ("scheduled", OperationAction::BuiltIn),
            ("executing", OperationAction::BuiltIn),
            ("successful", OperationAction::Clear),
            ("failed", OperationAction::Clear),
        ]
        .into_iter()
        .map(|(state, action)| (state.to_string(), action))
        .collect();

        OperationWorkflow {
            built_in: true,
            operation,
            handlers: DefaultHandlers::default(),
            states,
        }
    }

    /// Return the MQTT message to register support for the operation described by this workflow
    pub fn capability_message(
        &self,
        schema: &MqttSchema,
        target: &EntityTopicId,
    ) -> Option<MqttMessage> {
        match self.operation {
            // Custom operations (and restart) have a generic empty capability message
            OperationType::Custom(_) | OperationType::Restart => {
                let meta_topic = schema.capability_topic_for(target, self.operation.clone());
                let payload = "{}".to_string();
                Some(
                    MqttMessage::new(&meta_topic, payload)
                        .with_retain()
                        .with_qos(QoS::AtLeastOnce),
                )
            }
            // Builtin operations dynamically publish their capability message,
            // notably to include a list of supported types.
            _ => None,
        }
    }

    /// Return the action to be performed on a given state
    pub fn get_action(
        &self,
        command_state: &GenericCommandState,
    ) -> Result<OperationAction, WorkflowExecutionError> {
        self.states
            .get(&command_state.status)
            .ok_or_else(|| WorkflowExecutionError::UnknownStep {
                operation: (&self.operation).into(),
                step: command_state.status.clone(),
            })
            .map(|action| action.inject_state(command_state))
    }
}

impl OperationAction {
    pub fn with_default(self, default: &DefaultHandlers) -> Self {
        match self {
            OperationAction::Script(script, handlers) => {
                OperationAction::Script(script, handlers.with_default(default))
            }
            OperationAction::AwaitingAgentRestart(handlers) => {
                OperationAction::AwaitingAgentRestart(handlers.with_default(default))
            }
            OperationAction::AwaitOperationCompletion(handlers, state_excerpt) => {
                OperationAction::AwaitOperationCompletion(
                    handlers.with_default(default),
                    state_excerpt,
                )
            }
            action => action,
        }
    }

    pub fn inject_state(&self, state: &GenericCommandState) -> Self {
        match self {
            OperationAction::Script(script, handlers) => OperationAction::Script(
                Self::inject_values_into_script(state, script),
                handlers.clone(),
            ),
            OperationAction::BgScript(script, handlers) => OperationAction::BgScript(
                Self::inject_values_into_script(state, script),
                handlers.clone(),
            ),
            OperationAction::Operation(operation_expr, optional_script, input, handlers) => {
                let operation = state.inject_values_into_template(operation_expr);
                let optional_script = optional_script
                    .as_ref()
                    .map(|script| Self::inject_values_into_script(state, script));
                OperationAction::Operation(
                    operation,
                    optional_script,
                    input.clone(),
                    handlers.clone(),
                )
            }
            _ => self.clone(),
        }
    }

    fn inject_values_into_script(state: &GenericCommandState, script: &ShellScript) -> ShellScript {
        ShellScript {
            command: state.inject_values_into_template(&script.command),
            args: state.inject_values_into_parameters(&script.args),
        }
    }
}
