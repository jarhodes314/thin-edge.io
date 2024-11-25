use crate::cli::common::Cloud;
use anyhow::anyhow;
use camino::Utf8PathBuf;
use tedge_config::OptionalConfigError;
use tedge_config::ProfileName;

use super::create::CreateCertCmd;
use super::create_csr::CreateCsrCmd;
use super::remove::RemoveCertCmd;
use super::renew::RenewCertCmd;
use super::show::ShowCertCmd;
use super::upload::*;

use crate::command::BuildCommand;
use crate::command::BuildContext;
use crate::command::Command;
use crate::ConfigError;

#[derive(clap::Subcommand, Debug)]
pub enum TEdgeCertCli {
    /// Create a self-signed device certificate
    Create {
        /// The device identifier to be used as the common name for the certificate
        #[clap(long = "device-id")]
        id: String,
    },

    /// Create a certificate signing request
    CreateCsr {
        /// The device identifier to be used as the common name for the certificate
        #[clap(long = "device-id")]
        id: Option<String>,

        /// Path where a Certificate signing request will be stored
        #[clap(long = "output-path")]
        output_path: Option<Utf8PathBuf>,
    },

    /// Renew the device certificate
    Renew,

    /// Show the device certificate, if any
    Show,

    /// Remove the device certificate
    Remove,

    /// Upload root certificate
    Upload(UploadCertCli),
}

impl BuildCommand for TEdgeCertCli {
    fn build_command(self, context: BuildContext) -> Result<Box<dyn Command>, ConfigError> {
        let config = context.load_config()?;
        let (user, group) = if config.mqtt.bridge.built_in {
            ("tedge", "tedge")
        } else {
            (crate::BROKER_USER, crate::BROKER_USER)
        };

        let cmd = match self {
            TEdgeCertCli::Create { id } => {
                let cmd = CreateCertCmd {
                    id,
                    cert_path: config.device.cert_path.clone(),
                    key_path: config.device.key_path.clone(),
                    user: user.to_owned(),
                    group: group.to_owned(),
                };
                cmd.into_boxed()
            }

            TEdgeCertCli::CreateCsr { id, output_path } => {
                // Use the current device id if no id is provided
                let id = match id {
                    Some(id) => id,
                    None => config.device.id.try_read(&config)?.clone(),
                };
                let cmd = CreateCsrCmd {
                    id,
                    key_path: config.device.key_path.clone(),
                    // Use output file instead of csr_path from tedge config if provided
                    csr_path: output_path.unwrap_or_else(|| config.device.csr_path.clone()),
                    user: user.to_owned(),
                    group: group.to_owned(),
                };
                cmd.into_boxed()
            }

            TEdgeCertCli::Show => {
                let cmd = ShowCertCmd {
                    cert_path: config.device.cert_path.clone(),
                };
                cmd.into_boxed()
            }

            TEdgeCertCli::Remove => {
                let cmd = RemoveCertCmd {
                    cert_path: config.device.cert_path.clone(),
                    key_path: config.device.key_path.clone(),
                };
                cmd.into_boxed()
            }

            TEdgeCertCli::Upload(cmd) => {
                let cmd = match cmd.cloud {
                    Cloud::C8y(profile) => UploadCertCmd {
                        device_id: config.device.id.try_read(&config)?.clone(),
                        path: config.device.cert_path.clone(),
                        host: config
                            .c8y
                            .try_get(profile.as_deref())?
                            .http
                            .or_err()?
                            .to_owned(),
                        cloud_root_certs: config.cloud_root_certs(),
                        username: cmd.username,
                        password: cmd.password,
                    },
                    cloud => {
                        return Err(anyhow!(
                            "Uploading certificates via the tedge cli isn't supported for {cloud}"
                        )
                        .into())
                    }
                };
                cmd.into_boxed()
            }
            TEdgeCertCli::Renew => {
                let cmd = RenewCertCmd {
                    cert_path: config.device.cert_path.clone(),
                    key_path: config.device.key_path.clone(),
                };
                cmd.into_boxed()
            }
        };
        Ok(cmd)
    }
}

#[derive(clap::Args, Debug)]
pub struct UploadCertCli {
    cloud: Cloud,
    #[clap(long = "user")]
    #[arg(
        env = "C8Y_USER",
        hide_env_values = true,
        hide_default_value = true,
        default_value = ""
    )]
    /// Provided username should be a Cumulocity IoT user with tenant management permissions.
    /// You will be prompted for input if the value is not provided or is empty
    username: String,

    #[clap(long = "password")]
    #[arg(env = "C8Y_PASSWORD", hide_env_values = true, hide_default_value = true, default_value_t = std::env::var("C8YPASS").unwrap_or_default().to_string())]
    // Note: Prefer C8Y_PASSWORD over the now deprecated C8YPASS env variable as the former is also supported by other tooling such as go-c8y-cli
    /// Cumulocity IoT Password.
    /// You will be prompted for input if the value is not provided or is empty
    ///
    /// Notes: `C8YPASS` is deprecated. Please use the `C8Y_PASSWORD` env variable instead
    password: String,

    #[clap(long, hide = true)]
    profile: Option<ProfileName>,
}
