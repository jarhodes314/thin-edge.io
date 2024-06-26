use crate::cli::common::Cloud;
use crate::command::*;
use tedge_config::system_services::service_manager;

use super::command::ReconnectBridgeCommand;

use crate::bridge::AWS_CONFIG_FILENAME;
use crate::bridge::AZURE_CONFIG_FILENAME;
use crate::bridge::C8Y_CONFIG_FILENAME;

#[derive(clap::Subcommand, Debug)]
pub enum TEdgeReconnectCli {
    /// Remove bridge connection to Cumulocity.
    C8y,
    /// Remove bridge connection to Azure.
    Az,
    /// Remove bridge connection to AWS.
    Aws,
}

impl BuildCommand for TEdgeReconnectCli {
    fn build_command(self, context: BuildContext) -> Result<Box<dyn Command>, crate::ConfigError> {
        let config_location = context.config_location.clone();
        let config = context.load_config()?;
        let service_manager = service_manager(&context.config_location.tedge_config_root_path)?;

        let cmd = match self {
            TEdgeReconnectCli::C8y => ReconnectBridgeCommand {
                config_location,
                config,
                service_manager,
                config_file: C8Y_CONFIG_FILENAME.into(),
                cloud: Cloud::C8y,
                use_mapper: true,
            },
            TEdgeReconnectCli::Az => ReconnectBridgeCommand {
                config_location,
                config,
                service_manager,
                config_file: AZURE_CONFIG_FILENAME.into(),
                cloud: Cloud::Azure,
                use_mapper: true,
            },
            TEdgeReconnectCli::Aws => ReconnectBridgeCommand {
                config_location,
                config,
                service_manager,
                config_file: AWS_CONFIG_FILENAME.into(),
                cloud: Cloud::Aws,
                use_mapper: true,
            },
        };
        Ok(cmd.into_boxed())
    }
}
