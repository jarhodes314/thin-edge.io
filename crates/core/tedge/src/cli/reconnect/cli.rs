use super::command::ReconnectBridgeCommand;
use crate::cli::common::CloudArg;
use crate::command::*;
use crate::system_services::service_manager;
use tedge_config::TEdgeConfig;
use tedge_config::TEdgeConfigLocation;

#[derive(clap::Args, Debug)]
pub struct TEdgeReconnectCli {
    #[clap(subcommand)]
    cloud: CloudArg,
}

impl BuildCommand for TEdgeReconnectCli {
    fn build_command(
        self,
        config: TEdgeConfig,
        config_location: TEdgeConfigLocation,
    ) -> Result<Box<dyn Command>, crate::ConfigError> {
        Ok(ReconnectBridgeCommand {
            config,
            service_manager: service_manager(&config_location.tedge_config_root_path)?,
            config_location,
            cloud: self.cloud.try_into()?,
            use_mapper: true,
        }
        .into_boxed())
    }
}
