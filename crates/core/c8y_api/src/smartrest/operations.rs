use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::smartrest::error::OperationsError;
use crate::smartrest::smartrest_serializer::SmartRestSerializer;
use crate::smartrest::smartrest_serializer::SmartRestSetSupportedOperations;
use serde::Deserialize;

use super::error::SmartRestSerializerError;

/// Operations are derived by reading files subdirectories per cloud /etc/tedge/operations directory
/// Each operation is a file name in one of the subdirectories
/// The file name is the operation name

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OnMessageExec {
    command: Option<String>,
    on_message: Option<String>,
    topic: Option<String>,
    user: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub struct Operation {
    #[serde(skip)]
    pub name: String,
    exec: Option<OnMessageExec>,
}

impl Operation {
    pub fn exec(&self) -> Option<&OnMessageExec> {
        self.exec.as_ref()
    }

    pub fn command(&self) -> Option<String> {
        self.exec().and_then(|exec| exec.command.clone())
    }

    pub fn topic(&self) -> Option<String> {
        self.exec().and_then(|exec| exec.topic.clone())
    }
}

#[derive(Debug, Default, Clone)]
pub struct Operations {
    operations: Vec<Operation>,
    operations_by_trigger: HashMap<String, usize>,
}

/// depending on which editor you use, temporary files could be created that contain the name of
/// the file.
/// this `operation_name_is_valid` fn will ensure that only files that do not contain
/// any special characters are allowed.
pub fn is_valid_operation_name(operation: &str) -> bool {
    operation
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c.is_numeric() || c.eq(&'_'))
}

impl Operations {
    pub fn add_operation(&mut self, operation: Operation) {
        if self.operations.iter().any(|o| o.name.eq(&operation.name)) {
        } else {
            if let Some(detail) = operation.exec() {
                if let Some(on_message) = &detail.on_message {
                    self.operations_by_trigger
                        .insert(on_message.clone(), self.operations.len());
                }
            }
            self.operations.push(operation);
        }
    }

    pub fn remove_operation(&mut self, op_name: &str) {
        self.operations.retain(|x| x.name.ne(&op_name));
    }

    pub fn try_new(dir: impl AsRef<Path>) -> Result<Self, OperationsError> {
        get_operations(dir.as_ref())
    }

    pub fn get_child_ops(
        ops_dir: impl AsRef<Path>,
    ) -> Result<HashMap<String, Self>, OperationsError> {
        let mut child_ops: HashMap<String, Operations> = HashMap::new();
        let child_entries = fs::read_dir(&ops_dir)
            .map_err(|_| OperationsError::ReadDirError {
                dir: ops_dir.as_ref().into(),
            })?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<Vec<PathBuf>, _>>()?
            .into_iter()
            .filter(|path| path.is_dir())
            .collect::<Vec<PathBuf>>();
        for cdir in child_entries {
            let ops = Operations::try_new(&cdir)?;
            if let Some(id) = cdir.file_name() {
                if let Some(id_str) = id.to_str() {
                    child_ops.insert(id_str.to_string(), ops);
                }
            }
        }
        Ok(child_ops)
    }

    pub fn get_operations_list(&self) -> Vec<String> {
        self.operations
            .iter()
            .map(|operation| operation.name.clone())
            .collect::<Vec<String>>()
    }

    pub fn matching_smartrest_template(&self, operation_template: &str) -> Option<&Operation> {
        self.operations_by_trigger
            .get(operation_template)
            .and_then(|index| self.operations.get(*index))
    }

    pub fn topics_for_operations(&self) -> HashSet<String> {
        self.operations
            .iter()
            .filter_map(|operation| operation.topic())
            .collect::<HashSet<String>>()
    }

    pub fn create_smartrest_ops_message(&self) -> Result<String, SmartRestSerializerError> {
        let mut ops = self.get_operations_list();
        ops.sort();
        let ops = ops.iter().map(|op| op as &str).collect::<Vec<&str>>();
        SmartRestSetSupportedOperations::new(&ops).to_smartrest()
    }
}

fn get_operations(dir: impl AsRef<Path>) -> Result<Operations, OperationsError> {
    let mut operations = Operations::default();
    let dir_entries = fs::read_dir(&dir)
        .map_err(|_| OperationsError::ReadDirError {
            dir: dir.as_ref().to_path_buf(),
        })?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<PathBuf>, _>>()?
        .into_iter()
        .filter(|path| path.is_file())
        .collect::<Vec<PathBuf>>();

    for path in dir_entries {
        if let Some(file_name) = path.file_name().and_then(|file_name| file_name.to_str()) {
            if !is_valid_operation_name(file_name) {
                continue;
            }

            let mut details = match fs::read(&path) {
                Ok(bytes) => toml::from_slice::<Operation>(bytes.as_slice())
                    .map_err(|e| OperationsError::TomlError(path.to_path_buf(), e))?,

                Err(err) => return Err(OperationsError::FromIo(err)),
            };

            details.name = path
                .file_name()
                .and_then(|filename| filename.to_str())
                .ok_or_else(|| OperationsError::InvalidOperationName(path.to_owned()))?
                .to_owned();
            operations.add_operation(details);
        }
    }
    Ok(operations)
}

pub fn get_operation(path: PathBuf) -> Result<Operation, OperationsError> {
    let mut details = match fs::read(&path) {
        Ok(bytes) => toml::from_slice::<Operation>(bytes.as_slice())
            .map_err(|e| OperationsError::TomlError(path.to_path_buf(), e))?,

        Err(err) => return Err(OperationsError::FromIo(err)),
    };

    details.name = path
        .file_name()
        .and_then(|filename| filename.to_str())
        .ok_or_else(|| OperationsError::InvalidOperationName(path.to_owned()))?
        .to_owned();

    Ok(details)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use test_case::test_case;

    // Structs for state change with the builder pattern
    // Structs for Operations
    struct Ops(Vec<PathBuf>);
    struct NoOps;

    struct TestOperationsBuilder<O> {
        temp_dir: tempfile::TempDir,
        operations: O,
    }

    impl TestOperationsBuilder<NoOps> {
        fn new() -> Self {
            Self {
                temp_dir: tempfile::tempdir().unwrap(),
                operations: NoOps,
            }
        }
    }

    impl TestOperationsBuilder<NoOps> {
        fn with_operations(self, operations_count: usize) -> TestOperationsBuilder<Ops> {
            let Self { temp_dir, .. } = self;

            let mut operations = Vec::new();
            for i in 0..operations_count {
                let file_path = temp_dir.path().join(format!("operation{}", i));
                let mut file = fs::File::create(&file_path).unwrap();
                file.write_all(
                    br#"[exec]
                        command = "echo"
                        on_message = "511""#,
                )
                .unwrap();
                operations.push(file_path);
            }

            TestOperationsBuilder {
                operations: Ops(operations),
                temp_dir,
            }
        }
    }

    impl TestOperationsBuilder<Ops> {
        fn build(self) -> TestOperations {
            let Self {
                temp_dir,
                operations,
            } = self;

            TestOperations {
                temp_dir,
                operations: operations.0,
            }
        }
    }

    struct TestOperations {
        temp_dir: tempfile::TempDir,
        #[allow(dead_code)]
        operations: Vec<PathBuf>,
    }

    impl TestOperations {
        fn builder() -> TestOperationsBuilder<NoOps> {
            TestOperationsBuilder::new()
        }

        fn temp_dir(&self) -> &tempfile::TempDir {
            &self.temp_dir
        }
    }

    #[test_case(0)]
    #[test_case(1)]
    #[test_case(5)]
    fn get_operations_all(ops_count: usize) {
        let test_operations = TestOperations::builder().with_operations(ops_count).build();

        let operations = get_operations(test_operations.temp_dir()).unwrap();

        assert_eq!(operations.operations.len(), ops_count);
    }

    #[test_case("file_a?", false)]
    #[test_case("~file_b", false)]
    #[test_case("c8y_Command", true)]
    #[test_case("c8y_CommandA~", false)]
    #[test_case(".c8y_CommandB", false)]
    #[test_case("c8y_CommandD?", false)]
    #[test_case("c8y_CommandE?!£$%^&*(", false)]
    #[test_case("?!£$%^&*(c8y_CommandF?!£$%^&*(", false)]
    fn operation_name_should_contain_only_alphabetic_chars(operation: &str, expected_result: bool) {
        assert_eq!(is_valid_operation_name(operation), expected_result)
    }
}
