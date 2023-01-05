use c8y_api::json_c8y::*;
use std::path::PathBuf;
use tedge_actors::fan_in_message_type;

fan_in_message_type!(C8YRestRequest[C8yCreateEvent, C8yUpdateSoftwareListResponse, UploadLogBinary, UploadConfigFile]: Debug);
fan_in_message_type!(C8YRestResponse[EventId, Unit]: Debug);

#[derive(Debug)]
pub struct UploadLogBinary {
    pub log_type: String,
    pub log_content: String,
    pub child_device_id: Option<String>,
}

#[derive(Debug)]
pub struct UploadConfigFile {
    pub config_path: PathBuf,
    pub config_type: String,
    pub child_device_id: Option<String>,
}

pub type EventId = String;

pub type Unit = ();