## Goal

Simplify how thin-edge.io creates files and directories and sets their permissions, by separating the `PermissionEntry` data type from the behaviour that uses it, collapsing a proliferation of near-identical wrapper functions, and expressing the set of paths `tedge init` manages as a declarative table rather than imperative code — enabling unit tests for the initialisation logic in Rust.

## Decisions

### `PermissionEntry` becomes a pure data type

Remove `create_directory`, `create_file`, and `reassert_dir_ownership` from `PermissionEntry`. The struct holds desired permissions (user, group, mode); nothing more. `apply()` stays, as setting permissions on an existing path is the core purpose of the type.

The `reassert_dir_ownership` flag is the clearest symptom of the problem: it is a behaviour toggle embedded in a value type. Before:

```rust
pub struct PermissionEntry {
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u32>,
    pub reassert_dir_ownership: bool,  // behaviour toggle — doesn't belong here
}
```

After:

```rust
pub struct PermissionEntry {
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u32>,
}
```

The behaviour moves to the call site, expressed either as a parameter or a separate function name (see next decision).

### Collapse the wrapper function variants

Currently there are five public functions for creating a directory:

```rust
pub async fn create_directory(dir, permissions: &PermissionEntry)
pub async fn create_directory_and_update_ownership(dir, permissions: &PermissionEntry)
pub async fn create_directory_with_defaults(dir)
pub async fn create_directory_with_user_group(dir, user, group, mode)
pub async fn create_directory_with_user_group_and_update_ownership(dir, user, group, mode)
```

And four for files:

```rust
pub async fn create_file(file, content, permissions: PermissionEntry)
pub async fn create_file_with_defaults(file, content)
pub async fn create_file_with_mode(file, content, mode)
pub async fn create_file_with_user_group(file, user, group, mode, content)
```

The `_with_user_group` and `_with_mode` variants exist solely because callers didn't want to construct a `PermissionEntry` themselves. With a clean constructor, they're unnecessary. The `_with_defaults` variants can be replaced by `PermissionEntry::default()`. This reduces the surface to two functions each:

```rust
// Creates the directory; silently succeeds if it already exists
pub async fn ensure_dir(dir: impl AsRef<Path>, permissions: &PermissionEntry) -> Result<(), FileError>

// As above, but also reasserts ownership if the directory already exists
pub async fn ensure_dir_with_ownership(dir: impl AsRef<Path>, permissions: &PermissionEntry) -> Result<(), FileError>

// Creates the file with optional content; silently succeeds if it already exists
pub async fn ensure_file(file: impl AsRef<Path>, content: Option<&str>, permissions: &PermissionEntry) -> Result<(), FileError>
```

### Remove the empty-string sentinel convention from `permissions()`

The `permissions(user, group, mode)` free function treats `""` as `None`, which forces every caller to know about that convention:

```rust
// current — "" is secretly "don't set this field"
let p = file::permissions(&user, &group, 0o775);
let p = file::permissions("", "", 0o644);  // no ownership change
```

This is replaced with direct `PermissionEntry` construction. The type already has `Default` (all `None`) and `new(user, group, mode)`. For the common case of "set all three", callers use:

```rust
let p = PermissionEntry {
    user: Some(user),
    group: Some(group),
    mode: Some(0o775),
};
// or a constructor:
let p = PermissionEntry::owned(user, group, 0o775);
```

The `permissions()` free function is deleted.

### Declarative path table in `init.rs`

`initialize_tedge` currently contains a flat list of ~10 imperative calls:

```rust
create_directory_and_update_ownership(&config_dir, &permissions).await?;
create_directory_and_update_ownership(config_dir.join("mosquitto-conf"), &permissions).await?;
create_directory_and_update_ownership(config_dir.join("operations"), &permissions).await?;
// ... and so on
```

This cannot be unit-tested because it uses real paths from `TEdgeConfig` and calls real filesystem functions. The fix is to extract the "what paths should exist" knowledge into a data structure:

```rust
struct PathDef {
    path: PathBuf,
    kind: PathKind,
    permissions: PermissionEntry,
    update_existing: bool,
}

enum PathKind {
    Dir,
    File { default_content: Option<&'static str> },
}
```

`initialize_tedge` becomes: call `required_paths(config, &user, &group)` to get a `Vec<PathDef>`, then loop:

```rust
for path_def in required_paths(&config, &user, &group) {
    fs.ensure_path(&path_def).await?;
}
```

This unlocks two kinds of unit test:

1. **Assert on the table**: verify `required_paths` returns the expected paths with the expected permissions — no filesystem needed.
2. **Assert on the loop**: extend the existing `MockFileSystem` (already used for symlink tests in `init.rs`) to cover `ensure_path`, and test that the loop calls it correctly for each entry.

### Async/sync duplication is out of scope

The pattern of every async function having a `_sync` twin delegating to `spawn_blocking` is verbose but mechanical. Addressing it is a larger API change with limited unit-testability benefit; it is deferred.

## Non-goals / deferred

- **Changing which paths `tedge init` creates** — this is a pure refactor; no paths are added, removed, or permission-changed.
- **Certificate key permission handling** in `certificate/create.rs` — those are narrow, well-understood usages (`0o600`, `0o400`, `0o444` for key files) not part of the structural problem.
- **`DraftFile` / `atomic.rs` changes** — the atomic write utilities are separately coherent; not touched here.
- **Async/sync duplication** — deferred as noted above.

## Risks / trade-offs

The collapse of wrapper functions is a breaking change within the crate. All call sites (`init.rs`, `tedge_config_location.rs`, config/log managers, downloader, firmware manager) must be updated. The change is mechanical but touches many files; the risk is low because all callers are internal to this repository.

Renaming `create_directory` → `ensure_dir` makes the idempotent semantics explicit (it already silently succeeds on `AlreadyExists`), but is a larger rename diff. If preferred, the old names can be kept with deprecation notices in a follow-up.

## Capabilities

### New Capabilities
- `permission-entry`: The simplified `PermissionEntry` data type, its constructors, and the reduced set of file/directory creation helpers that accept it
- `path-initialisation`: The declarative `PathDef` table pattern for `tedge init`, and the extended `FileSystem` trait that makes the initialisation loop unit-testable

### Modified Capabilities
<!-- none — no existing spec requirements are changing -->
