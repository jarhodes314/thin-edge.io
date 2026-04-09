## ADDED Requirements

### Requirement: PermissionEntry is a pure data type

`PermissionEntry` SHALL hold only `user: Option<String>`, `group: Option<String>`, and `mode: Option<u32>`. It SHALL NOT carry behavioural flags or provide methods that create or modify filesystem entries (other than `apply`).

#### Scenario: PermissionEntry has no reassert_dir_ownership field

- **WHEN** a developer constructs a `PermissionEntry`
- **THEN** the struct has exactly three fields: `user`, `group`, and `mode`

#### Scenario: PermissionEntry::default produces an all-None entry

- **WHEN** `PermissionEntry::default()` is called
- **THEN** all three fields are `None`

---

### Requirement: PermissionEntry::owned constructor

`PermissionEntry` SHALL provide an `owned(user: String, group: String, mode: u32)` constructor that sets all three fields to `Some(...)`.

#### Scenario: owned sets all fields

- **WHEN** `PermissionEntry::owned("tedge".into(), "tedge".into(), 0o775)` is called
- **THEN** the resulting entry has `user = Some("tedge")`, `group = Some("tedge")`, `mode = Some(0o775)`

---

### Requirement: apply sets ownership and mode on an existing path

`PermissionEntry::apply(path)` SHALL change the ownership and/or mode of an existing file or directory according to the non-`None` fields. Fields that are `None` SHALL be left unchanged.

#### Scenario: apply changes mode

- **WHEN** a file exists with mode `0o644` and `apply` is called with `mode = Some(0o444)`
- **THEN** the file's mode is `0o444`

#### Scenario: apply with None mode leaves mode unchanged

- **WHEN** a file exists with mode `0o644` and `apply` is called with `mode = None`
- **THEN** the file's mode remains `0o644`

#### Scenario: apply with unknown user returns an error

- **WHEN** `apply` is called with `user = Some("nonexistent_user_xyz")`
- **THEN** an error is returned containing "User not found"

---

### Requirement: ensure_dir creates a directory with given permissions

`ensure_dir(path, permissions)` SHALL create the directory (and any missing parents) and apply the given `PermissionEntry`. If the directory already exists it SHALL succeed without modifying ownership or mode.

#### Scenario: creates missing directory

- **WHEN** the path does not exist and `ensure_dir` is called with a `PermissionEntry`
- **THEN** the directory exists and has the specified mode

#### Scenario: idempotent on existing directory

- **WHEN** the directory already exists and `ensure_dir` is called again
- **THEN** the call succeeds and the existing permissions are unchanged

---

### Requirement: ensure_dir_with_ownership reasserts ownership on existing directories

`ensure_dir_with_ownership(path, permissions)` SHALL behave like `ensure_dir` but SHALL also call `apply` on the directory even when it already exists.

#### Scenario: updates ownership on existing directory

- **WHEN** a directory exists owned by a different user and `ensure_dir_with_ownership` is called with a specific user
- **THEN** the directory's owner is updated to the specified user

---

### Requirement: ensure_file creates a file with given permissions

`ensure_file(path, content, permissions)` SHALL create the file with the given default content and apply the given `PermissionEntry`. If the file already exists it SHALL succeed without modifying the content or permissions.

#### Scenario: creates missing file with content

- **WHEN** the path does not exist and `ensure_file` is called with content `Some("hello")`
- **THEN** the file exists with content `"hello"` and the specified mode

#### Scenario: idempotent on existing file

- **WHEN** the file already exists with different content and `ensure_file` is called
- **THEN** the call succeeds and the existing content is unchanged

---

### Requirement: No empty-string sentinel in public API

The `permissions(user, group, mode)` free function that treats `""` as `None` SHALL be removed. Callers SHALL construct `PermissionEntry` directly.

#### Scenario: no permissions() free function

- **WHEN** a developer searches the public API of `tedge_utils::file`
- **THEN** there is no function named `permissions` that accepts raw `&str` arguments and maps empty strings to `None`
