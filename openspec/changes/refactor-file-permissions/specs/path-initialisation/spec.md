## ADDED Requirements

### Requirement: required_paths returns a declarative list of all paths managed by tedge init

A function `required_paths(config: &TEdgeConfig, user: &str, group: &str) -> Vec<PathDef>` SHALL return the complete set of filesystem entries that `tedge init` is responsible for creating. Each entry SHALL capture: the path, whether it is a directory or file (with optional default content), the desired `PermissionEntry`, and whether ownership should be reasserted on existing paths.

#### Scenario: config directory is in the required paths with 0o775 and update_existing

- **WHEN** `required_paths` is called with a config whose `root_dir` is `/etc/tedge`
- **THEN** the returned list contains an entry for `/etc/tedge` with `mode = Some(0o775)` and `update_existing = true`

#### Scenario: required_paths includes all expected subdirectories

- **WHEN** `required_paths` is called
- **THEN** the returned list contains entries for `mosquitto-conf`, `operations`, `operations/c8y`, `plugins`, `sm-plugins`, `device-certs`, `mappers`, `.tedge-mapper-c8y`, the configured log directory, and the configured data directory

#### Scenario: required_paths includes the agent state directory

- **WHEN** `required_paths` is called and `config.agent.state.path` does not exist on disk
- **THEN** the returned list contains an entry for `.agent` under the config directory

#### Scenario: config files are included with mode 0o644

- **WHEN** `required_paths` is called
- **THEN** entries for files such as `system.toml` and `entity_store.jsonl` have `mode = Some(0o644)`

---

### Requirement: initialize_tedge iterates required_paths via the FileSystem trait

The `initialize_tedge` function SHALL obtain the path list from `required_paths` and apply each entry by calling a method on the `FileSystem` trait, rather than calling filesystem functions directly.

#### Scenario: ensure_path called for each PathDef

- **WHEN** `initialize_tedge` is called with a mock `FileSystem`
- **THEN** `fs.ensure_path` is called exactly once for each entry returned by `required_paths`

#### Scenario: ensure_path called with correct permissions for a directory entry

- **WHEN** a `PathDef` has `kind = Dir`, `permissions.mode = Some(0o775)`, and `update_existing = true`
- **THEN** `fs.ensure_path` receives that `PathDef` and calls `ensure_dir_with_ownership` internally

---

### Requirement: required_paths is unit-testable without a filesystem

`required_paths` SHALL be a pure function of its inputs — it SHALL NOT read from or write to the filesystem. All path construction SHALL derive solely from the `TEdgeConfig` values and the `user`/`group` strings passed in.

#### Scenario: required_paths does not fail when paths do not exist

- **WHEN** `required_paths` is called with a config referencing paths that do not exist on disk
- **THEN** it returns successfully with the expected list of `PathDef` entries

#### Scenario: path list can be asserted in a unit test

- **WHEN** a unit test constructs a `TEdgeConfig` with known paths and calls `required_paths`
- **THEN** the test can assert on the contents of the returned `Vec<PathDef>` without performing any filesystem operations
