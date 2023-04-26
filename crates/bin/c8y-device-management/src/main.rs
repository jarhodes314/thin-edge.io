use c8y_config_manager::ConfigManagerBuilder;
use c8y_config_manager::ConfigManagerConfig;
use c8y_firmware_manager::FirmwareManagerBuilder;
use c8y_firmware_manager::FirmwareManagerConfig;
use c8y_http_proxy::credentials::C8YJwtRetriever;
use c8y_http_proxy::C8YHttpProxyBuilder;
use c8y_log_manager::LogManagerBuilder;
use c8y_log_manager::LogManagerConfig;
use tedge_actors::Runtime;
use tedge_config::get_tedge_config;
use tedge_config::DEFAULT_TEDGE_CONFIG_PATH;
use tedge_downloader_ext::DownloaderActor;
use tedge_file_system_ext::FsWatchActorBuilder;
use tedge_health_ext::HealthMonitorBuilder;
use tedge_http_ext::HttpActor;
use tedge_mqtt_ext::MqttActorBuilder;
use tedge_signal_ext::SignalActor;
use tedge_timer_ext::TimerActor;

pub const PLUGIN_NAME: &str = "c8y-device-management";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let runtime_events_logger = None;
    let mut runtime = Runtime::try_new(runtime_events_logger).await?;

    let tedge_config = get_tedge_config()?;
    let c8y_http_config = (&tedge_config).try_into()?;
    let mqtt_config = tedge_config.mqtt_config()?;

    // Create actor instances
    let mut mqtt_actor = MqttActorBuilder::new(mqtt_config.clone().with_session_name(PLUGIN_NAME));
    let mut jwt_actor = C8YJwtRetriever::builder(mqtt_config);
    let mut http_actor = HttpActor::new().builder();

    let mut c8y_http_proxy_actor =
        C8YHttpProxyBuilder::new(c8y_http_config, &mut http_actor, &mut jwt_actor);

    let mut fs_watch_actor = FsWatchActorBuilder::new();
    let mut timer_actor = TimerActor::builder();
    let mut downloader_actor = DownloaderActor::new().builder();

    // Instantiate config manager actor
    let config_manager_config =
        ConfigManagerConfig::from_tedge_config(DEFAULT_TEDGE_CONFIG_PATH, &tedge_config)?;
    let config_actor = ConfigManagerBuilder::new(
        config_manager_config,
        &mut mqtt_actor,
        &mut c8y_http_proxy_actor,
        &mut timer_actor,
        &mut fs_watch_actor,
    );

    // Instantiate log manager actor
    let log_manager_config =
        LogManagerConfig::from_tedge_config(DEFAULT_TEDGE_CONFIG_PATH, &tedge_config)?;
    let log_actor = LogManagerBuilder::new(
        log_manager_config,
        &mut mqtt_actor,
        &mut c8y_http_proxy_actor,
        &mut fs_watch_actor,
    );

    // Instantiate firmware manager actor
    let firmware_manager_config = FirmwareManagerConfig::from_tedge_config(&tedge_config)?;
    let firmware_actor = FirmwareManagerBuilder::new(
        firmware_manager_config,
        &mut mqtt_actor,
        &mut jwt_actor,
        &mut timer_actor,
        &mut downloader_actor,
    );

    // Instantiate health monitor actor
    let health_actor = HealthMonitorBuilder::new(PLUGIN_NAME, &mut mqtt_actor);

    // Shutdown on SIGINT
    let signal_actor = SignalActor::builder(&runtime.get_handle());

    // Run the actors
    // FIXME: having to list all the actors is error prone
    runtime.spawn(signal_actor).await?;
    runtime.spawn(mqtt_actor).await?;
    runtime.spawn(jwt_actor).await?;
    runtime.spawn(http_actor).await?;
    runtime.spawn(c8y_http_proxy_actor).await?;
    runtime.spawn(fs_watch_actor).await?;
    runtime.spawn(config_actor).await?;
    runtime.spawn(log_actor).await?;
    runtime.spawn(firmware_actor).await?;
    runtime.spawn(timer_actor).await?;
    runtime.spawn(health_actor).await?;
    runtime.spawn(downloader_actor).await?;

    runtime.run_to_completion().await?;

    Ok(())
}
