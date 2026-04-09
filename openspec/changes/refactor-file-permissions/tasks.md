## PermissionEntry cleanup

- [x] Remove `reassert_dir_ownership` field from `PermissionEntry`
- [x] Remove `force_dir_ownership()` method from `PermissionEntry`
- [x] Remove `create_directory` method from `PermissionEntry`
- [x] Remove `create_file` method from `PermissionEntry`
- [x] Add `PermissionEntry::owned(user: String, group: String, mode: u32)` constructor
- [x] Delete the `permissions(user: &str, group: &str, mode: u32)` free function

## File/directory helper API

- [x] Rename `create_directory` → `ensure_dir`, taking `&PermissionEntry`
- [x] Rename `create_directory_and_update_ownership` → `ensure_dir_with_ownership`, taking `&PermissionEntry`
- [x] Delete `create_directory_with_defaults` (replace with `PermissionEntry::default()` at call sites)
- [x] Delete `create_directory_with_user_group` (replace with `PermissionEntry::owned` at call sites)
- [x] Delete `create_directory_with_user_group_and_update_ownership` (replace with `PermissionEntry::owned` at call sites)
- [x] Rename `create_file` → `ensure_file`, taking `&PermissionEntry`
- [x] Delete `create_file_with_defaults` (replace at call sites)
- [x] Delete `create_file_with_mode` (replace at call sites)
- [x] Delete `create_file_with_user_group` (replace at call sites)

## Update call sites

- [x] Update `crates/core/tedge/src/cli/init.rs` to use `ensure_dir` / `ensure_dir_with_ownership` and `PermissionEntry::owned`
- [x] Update `crates/common/tedge_config/src/tedge_toml/tedge_config_location.rs` to use new API
- [x] Update `crates/extensions/tedge_config_manager/src/lib.rs` to use new API
- [x] Update `crates/extensions/tedge_log_manager/src/lib.rs` to use new API
- [x] Update `crates/extensions/tedge_downloader_ext/src/actor.rs` to use new API
- [x] Update `crates/extensions/c8y_firmware_manager/src/worker.rs` to use new API
- [x] Update tests in `crates/common/tedge_utils/src/file.rs` to use new function names

## Declarative path table in `init.rs`

- [x] Define `PathDef` struct with `path`, `kind` (`PathKind::Dir` / `PathKind::File`), `permissions`, `update_existing`
- [x] Define `PathKind` enum
- [x] Implement `required_paths(config: &TEdgeConfig, user: &str, group: &str) -> Vec<PathDef>`
- [x] Move all directory/file entries from `initialize_tedge` into `required_paths`
- [x] Extend the `FileSystem` trait with an `ensure_path(&PathDef)` method
- [x] Rewrite `initialize_tedge` to call `fs.ensure_path` for each entry from `required_paths`
- [x] Implement `ensure_path` on `RealEnv`

## Unit tests

- [x] Add unit test asserting `required_paths` returns config dir with `mode = 0o775` and `update_existing = true`
- [x] Add unit test asserting all expected subdirectory names are present in `required_paths`
- [x] Add unit test asserting file entries (e.g. `system.toml`) have `mode = 0o644`
- [x] Add mock-based unit test verifying `initialize_tedge` calls `ensure_path` for each `PathDef`
