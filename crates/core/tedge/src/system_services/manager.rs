use camino::Utf8Path;

use super::*;
use std::fmt::Debug;
use std::sync::Arc;
use tedge_config::SystemTomlError;

/// Abstraction over the system-provided facility that manages starting, stopping as well as other
/// service-related management functions of system services.
#[async_trait::async_trait]
pub trait SystemServiceManager: Debug + Send + Sync {
    /// Returns the name of the system service manager facility (e.g. "systemd" or "openrc").
    fn name(&self) -> &str;

    /// Checks whether the system service manager facility is available and operational.
    async fn check_operational(&self) -> Result<(), SystemServiceError>;

    /// Stops the specified system service.
    async fn stop_service(&self, service: SystemService<'_>) -> Result<(), SystemServiceError>;

    /// Starts the specified system service.
    async fn start_service(&self, service: SystemService<'_>) -> Result<(), SystemServiceError>;

    /// Restarts the specified system service.
    async fn restart_service(&self, service: SystemService<'_>) -> Result<(), SystemServiceError>;

    /// Enables the specified system service. This does not start the service, unless you reboot.
    async fn enable_service(&self, service: SystemService<'_>) -> Result<(), SystemServiceError>;

    /// Disables the specified system service. This does not stop the service.
    async fn disable_service(&self, service: SystemService<'_>) -> Result<(), SystemServiceError>;

    /// Queries status of the specified system service. "Running" here means the same as "active".
    async fn is_service_running(
        &self,
        service: SystemService<'_>,
    ) -> Result<bool, SystemServiceError>;
}

pub fn service_manager(
    config_root: &Utf8Path,
) -> Result<Arc<dyn SystemServiceManager>, SystemTomlError> {
    Ok(Arc::new(GeneralServiceManager::try_new(config_root)?))
}
