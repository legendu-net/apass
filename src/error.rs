//! Error type for `apass`.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Yaml(#[from] serde_yaml_ng::Error),

    #[error(transparent)]
    Base64(#[from] base64::DecodeError),

    #[error(transparent)]
    Expect(#[from] expectrl::Error),

    #[error("Error: the HOME environment variable is not set")]
    HomeNotSet,

    #[error(
        "Error: could not determine the current username (LOGNAME/USER/LNAME/USERNAME are all unset)"
    )]
    UsernameNotSet,

    #[error("Error: cached password in {path} is not valid UTF-8")]
    InvalidCachedPassword { path: PathBuf },

    #[error("Error: cached password timestamp {timestamp:?} in {path} could not be parsed")]
    InvalidCacheTimestamp { path: PathBuf, timestamp: String },

    #[error(
        "Error: {path} uses the old mapping format. Each prompt is now an entry with a \
         command (a list of tokens) and a prompt, e.g.\n  - command: [\"ssh\", \"dev-server\"]\n    \
         prompt: \"{{USER}}@dev-server's password:\""
    )]
    OldMappingFormat { path: PathBuf },

    #[error("Error: {path} must contain a list of entries.")]
    PromptsNotList { path: PathBuf },

    #[error(
        "Error: Invalid entry {entry} in {path}; each entry needs a non-empty command \
         (a list of tokens) and a prompt."
    )]
    InvalidEntry { entry: String, path: PathBuf },

    #[error("Error: invalid placeholder in prompt template {template:?}")]
    InvalidPlaceholder { template: String },

    #[error("Error: No prompt whose command is a prefix of '{command}' found in {path}")]
    NoPromptFound { command: String, path: PathBuf },

    #[error(
        "Error: no password named {name:?} is cached in {path}. Run: apass password set {name}"
    )]
    PasswordNotFound { name: String, path: PathBuf },

    #[error(
        "Error: {path} uses the old single-password format. Passwords are now stored by name, \
         e.g.\n  {{\n    \"default\": {{\"password\": \"...\", \"time\": \"...\"}}\n  }}\nRun: \
         apass password set to recreate it."
    )]
    OldProfileFormat { path: PathBuf },

    #[error("Error: no password named {name:?} in {path}")]
    PasswordEntryNotFound { name: String, path: PathBuf },

    #[error("Error: no prompt entry found whose command is exactly '{command}' in {path}")]
    PromptEntryNotFound { command: String, path: PathBuf },

    #[error("Error: a prompt entry for '{command}' already exists in {path}")]
    PromptEntryExists { command: String, path: PathBuf },

    #[error("Error: editor {editor:?} exited with status {status}")]
    EditorFailed {
        editor: String,
        status: std::process::ExitStatus,
    },
}
