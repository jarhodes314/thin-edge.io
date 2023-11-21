use crate::state_repository::error::StateError;
use camino::Utf8PathBuf;
use serde::Deserialize;
use serde::Serialize;
use tedge_utils::fs::atomically_write_file_async;
use tokio::fs;

#[derive(Debug)]
pub struct AgentStateRepository {
    pub state_repo_path: Utf8PathBuf,
}

impl AgentStateRepository {
    pub fn state_dir(tedge_root: Utf8PathBuf) -> Utf8PathBuf {
        tedge_root.join(".agent")
    }

    pub fn new(tedge_root: Utf8PathBuf, file_name: &str) -> Self {
        let state_repo_path = Self::state_dir(tedge_root).join(file_name);

        Self { state_repo_path }
    }

    pub async fn load(&self) -> Result<State, StateError> {
        let text = fs::read_to_string(&self.state_repo_path)
            .await
            .map_err(|e| StateError::LoadingFromFileFailed {
                path: self.state_repo_path.as_path().into(),
                source: e,
            })?;

        let state = toml::from_str::<State>(&text).map_err(|e| StateError::FromTOMLParse {
            path: self.state_repo_path.as_path().into(),
            source: e,
        })?;

        Ok(state)
    }

    pub async fn store(&self, state: &State) -> Result<(), StateError> {
        let toml = toml::to_string_pretty(&state)?;
        atomically_write_file_async(&self.state_repo_path, toml.as_bytes()).await?;

        Ok(())
    }

    pub async fn clear(&self) -> Result<(), StateError> {
        let state = State {
            operation_id: None,
            operation: None,
        };
        self.store(&state).await?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum StateStatus {
    Software(SoftwareOperationVariants),
    Restart(RestartOperationStatus),
    UnknownOperation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SoftwareOperationVariants {
    List,
    Update,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RestartOperationStatus {
    Pending,
    Restarting,
}

#[derive(Debug, Default, Deserialize, Eq, PartialEq, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub operation_id: Option<String>,
    pub operation: Option<StateStatus>,
}

#[cfg(test)]
mod tests {
    use crate::state_repository::state::AgentStateRepository;
    use crate::state_repository::state::RestartOperationStatus;
    use crate::state_repository::state::SoftwareOperationVariants;
    use crate::state_repository::state::State;
    use crate::state_repository::state::StateStatus;

    use tedge_test_utils::fs::TempTedgeDir;

    #[tokio::test]
    async fn agent_state_repository_not_exists_fail() {
        let temp_dir = TempTedgeDir::new();
        let repo = AgentStateRepository::new(temp_dir.utf8_path_buf(), "current-operation");

        repo.load().await.unwrap_err();
    }

    #[tokio::test]
    async fn agent_state_repository_exists_loads_some() {
        let temp_dir = TempTedgeDir::new();
        let content = "operation_id = \'1234\'\noperation = \"list\"";
        temp_dir
            .dir(".agent")
            .file("current-operation")
            .with_raw_content(content);

        let repo = AgentStateRepository::new(temp_dir.utf8_path_buf(), "current-operation");

        let data = repo.load().await.unwrap();
        assert_eq!(
            data,
            State {
                operation_id: Some("1234".into()),
                operation: Some(StateStatus::Software(SoftwareOperationVariants::List)),
            }
        );
    }

    #[tokio::test]
    async fn agent_state_repository_exists_loads_some_restart_variant() {
        let temp_dir = TempTedgeDir::new();
        let content = "operation_id = \'1234\'\noperation = \"Restarting\"";
        temp_dir
            .dir(".agent")
            .file("current-operation")
            .with_raw_content(content);

        let repo = AgentStateRepository::new(temp_dir.utf8_path_buf(), "current-operation");

        let data = repo.load().await.unwrap();
        assert_eq!(
            data,
            State {
                operation_id: Some("1234".into()),
                operation: Some(StateStatus::Restart(RestartOperationStatus::Restarting)),
            }
        );
    }

    #[tokio::test]
    async fn agent_state_repository_exists_loads_none() {
        let temp_dir = TempTedgeDir::new();
        let content = "";
        temp_dir
            .dir(".agent")
            .file("current-operation")
            .with_raw_content(content);

        let repo = AgentStateRepository::new(temp_dir.utf8_path_buf(), "current-operation");

        let data = repo.load().await.unwrap();
        assert_eq!(
            data,
            State {
                operation_id: None,
                operation: None
            }
        );
    }

    #[tokio::test]
    async fn agent_state_repository_exists_store() {
        let temp_dir = TempTedgeDir::new();
        temp_dir.dir(".agent").file("current-operation");

        let repo = AgentStateRepository::new(temp_dir.utf8_path_buf(), "current-operation");

        repo.store(&State {
            operation_id: Some("1234".into()),
            operation: Some(StateStatus::Software(SoftwareOperationVariants::List)),
        })
        .await
        .unwrap();

        let data = tokio::fs::read_to_string(&format!(
            "{}/.agent/current-operation",
            &temp_dir.temp_dir.path().to_str().unwrap()
        ))
        .await
        .unwrap();

        assert_eq!(data, "operation_id = \"1234\"\noperation = \"list\"\n");
    }
}
