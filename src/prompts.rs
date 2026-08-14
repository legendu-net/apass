//! Load, query, and manage `prompts.yml`.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_yaml_ng::Value;

use crate::error::AppError;
use crate::password::DEFAULT_NAME;

/// A single configured prompt: the command prefix it matches, the (already
/// `{USER}`-substituted) regex for the password prompt that command emits,
/// and the name of the cached password to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub command: Vec<String>,
    pub prompt: String,
    pub password: String,
}

/// Load the raw YAML entries in `prompts.yml`, creating an empty file if it
/// doesn't exist yet. Unlike [`load`], this applies no `{USER}` substitution
/// and no per-entry validation, so callers that rewrite the file (`prompt
/// add`/`remove`) can round-trip entries they don't otherwise understand.
pub fn load_raw(path: &Path) -> Result<Vec<Value>, AppError> {
    if !path.is_file() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_yaml_ng::to_string(&Vec::<Value>::new())?)?;
    }

    let text = fs::read_to_string(path)?;
    let value: Value = serde_yaml_ng::from_str(&text)?;
    // An empty file parses as `Null`; treat that the same as an empty list.
    let value = if value.is_null() {
        Value::Sequence(Vec::new())
    } else {
        value
    };

    if value.is_mapping() {
        return Err(AppError::OldMappingFormat {
            path: path.to_path_buf(),
        });
    }
    let Value::Sequence(items) = value else {
        return Err(AppError::PromptsNotList {
            path: path.to_path_buf(),
        });
    };
    Ok(items)
}

fn save_raw(path: &Path, items: &[Value]) -> Result<(), AppError> {
    fs::write(path, serde_yaml_ng::to_string(&items)?)?;
    Ok(())
}

/// Load `prompts.yml`, creating an empty one if it doesn't exist yet.
pub fn load(path: &Path, user: &str) -> Result<Vec<Prompt>, AppError> {
    let items = load_raw(path)?;
    let mut prompts = Vec::with_capacity(items.len());
    for item in items {
        let entry = parse_entry(&item, user).ok_or_else(|| AppError::InvalidEntry {
            entry: format_entry(&item),
            path: path.to_path_buf(),
        })?;
        prompts.push(entry);
    }
    Ok(prompts)
}

fn parse_entry(item: &Value, user: &str) -> Option<Prompt> {
    let map = item.as_mapping()?;
    let command = map.get("command")?.as_sequence()?;
    if command.is_empty() {
        return None;
    }
    let command = command
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect::<Option<Vec<String>>>()?;
    let prompt = map.get("prompt")?.as_str()?;
    let prompt = substitute_user(prompt, user).ok()?;
    let password = match map.get("password") {
        None => DEFAULT_NAME.to_string(),
        Some(v) => v.as_str()?.to_string(),
    };
    Some(Prompt {
        command,
        prompt,
        password,
    })
}

fn entry_command(item: &Value) -> Option<Vec<String>> {
    item.as_mapping()?
        .get("command")?
        .as_sequence()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn format_entry(item: &Value) -> String {
    serde_yaml_ng::to_string(item)
        .map(|s| s.trim().replace('\n', " "))
        .unwrap_or_else(|_| format!("{item:?}"))
}

/// Substitutes `{USER}` in a `prompts.yml` template with the current
/// username. `{{`/`}}` become literal braces, and any other placeholder (or
/// an unmatched brace) is an error.
pub fn substitute_user(template: &str, user: &str) -> Result<String, AppError> {
    let invalid = || AppError::InvalidPlaceholder {
        template: template.to_string(),
    };

    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                    continue;
                }
                let mut field = String::new();
                let mut closed = false;
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        closed = true;
                        break;
                    }
                    field.push(c2);
                }
                if !closed {
                    return Err(invalid());
                }
                match field.as_str() {
                    "USER" => out.push_str(user),
                    _ => return Err(invalid()),
                }
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    out.push('}');
                } else {
                    return Err(invalid());
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// Find the prompt whose command is the longest prefix of `command`.
pub fn find_prompt<'a>(command: &[String], prompts: &'a [Prompt]) -> Option<&'a Prompt> {
    let mut best_length = 0;
    let mut best_prompt = None;
    for p in prompts {
        if p.command.len() > best_length
            && command.get(..p.command.len()) == Some(p.command.as_slice())
        {
            best_length = p.command.len();
            best_prompt = Some(p);
        }
    }
    best_prompt
}

/// `apass prompt list`: print each entry's command, prompt regex, and
/// password name (raw, i.e. before `{USER}` substitution).
pub fn cmd_list(path: &Path) -> Result<(), AppError> {
    let items = load_raw(path)?;
    for item in &items {
        let command = entry_command(item).unwrap_or_default().join(" ");
        let prompt = item
            .as_mapping()
            .and_then(|m| m.get("prompt"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let password = item
            .as_mapping()
            .and_then(|m| m.get("password"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_NAME);
        println!("{command}\t{prompt}\t{password}");
    }
    Ok(())
}

/// `apass prompt add`: append a new entry. Errors if an entry with the exact
/// same command already exists, or if `regex` fails `{USER}` substitution
/// (checked eagerly so a bad template is caught here rather than on the
/// next `run`).
pub fn cmd_add(
    path: &Path,
    command: &[String],
    regex: &str,
    password: &str,
    user: &str,
) -> Result<(), AppError> {
    substitute_user(regex, user)?;

    let mut items = load_raw(path)?;
    if items
        .iter()
        .any(|item| entry_command(item).as_deref() == Some(command))
    {
        return Err(AppError::PromptEntryExists {
            command: command.join(" "),
            path: path.to_path_buf(),
        });
    }

    let mut map = serde_yaml_ng::Mapping::new();
    map.insert(
        Value::String("command".to_string()),
        Value::Sequence(command.iter().cloned().map(Value::String).collect()),
    );
    map.insert(
        Value::String("prompt".to_string()),
        Value::String(regex.to_string()),
    );
    if password != DEFAULT_NAME {
        map.insert(
            Value::String("password".to_string()),
            Value::String(password.to_string()),
        );
    }
    items.push(Value::Mapping(map));
    save_raw(path, &items)
}

/// `apass prompt remove`: delete the entry whose command is exactly
/// `command`.
pub fn cmd_remove(path: &Path, command: &[String]) -> Result<(), AppError> {
    let mut items = load_raw(path)?;
    let before = items.len();
    items.retain(|item| entry_command(item).as_deref() != Some(command));
    if items.len() == before {
        return Err(AppError::PromptEntryNotFound {
            command: command.join(" "),
            path: path.to_path_buf(),
        });
    }
    save_raw(path, &items)
}

/// `apass prompt edit`: open `prompts.yml` in `$VISUAL`, `$EDITOR`, or `vi`,
/// then validate it. The file is left exactly as the user saved it -- even
/// if it's now invalid -- so a bad edit is reported, not silently reverted.
pub fn cmd_edit(path: &Path, user: &str) -> Result<(), AppError> {
    // Touch the file into existence first so there's something to edit.
    load_raw(path)?;

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor).arg(path).status()?;
    if !status.success() {
        return Err(AppError::EditorFailed { editor, status });
    }

    load(path, user)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|t| t.to_string()).collect()
    }

    fn prompt(command: &[&str], text: &str, password: &str) -> Prompt {
        Prompt {
            command: toks(command),
            prompt: text.to_string(),
            password: password.to_string(),
        }
    }

    #[test]
    fn missing_file_is_created_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        let prompts = load(&path, "alice").unwrap();
        assert!(prompts.is_empty());
        assert!(path.is_file());
    }

    #[test]
    fn valid_file_is_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(
            &path,
            r#"
- command: ["ssh", "dev-server"]
  prompt: "{USER}@dev-server's password:"
- command: ["sudo"]
  prompt: "[sudo] password for {USER}:"
"#,
        )
        .unwrap();
        let prompts = load(&path, "alice").unwrap();
        assert_eq!(
            prompts,
            vec![
                prompt(
                    &["ssh", "dev-server"],
                    "alice@dev-server's password:",
                    DEFAULT_NAME
                ),
                prompt(&["sudo"], "[sudo] password for alice:", DEFAULT_NAME),
            ]
        );
    }

    #[test]
    fn password_key_is_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(
            &path,
            "- command: [\"ssh\", \"work\"]\n  prompt: \"Password:\"\n  password: work\n",
        )
        .unwrap();
        let prompts = load(&path, "alice").unwrap();
        assert_eq!(prompts[0].password, "work");
    }

    #[test]
    fn missing_password_key_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(&path, "- command: [\"ssh\"]\n  prompt: \"Password:\"\n").unwrap();
        let prompts = load(&path, "alice").unwrap();
        assert_eq!(prompts[0].password, DEFAULT_NAME);
    }

    #[test]
    fn non_string_password_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(
            &path,
            "- command: [\"ssh\"]\n  prompt: \"Password:\"\n  password: [1, 2]\n",
        )
        .unwrap();
        let err = load(&path, "alice").unwrap_err();
        assert!(matches!(err, AppError::InvalidEntry { .. }));
    }

    #[test]
    fn old_mapping_format_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(&path, "ssh: \"password:\"\n").unwrap();
        let err = load(&path, "alice").unwrap_err();
        assert!(matches!(err, AppError::OldMappingFormat { .. }));
        assert!(err.to_string().contains("old mapping format"));
    }

    #[test]
    fn non_list_top_level_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(&path, "42\n").unwrap();
        let err = load(&path, "alice").unwrap_err();
        assert!(matches!(err, AppError::PromptsNotList { .. }));
    }

    #[test]
    fn entry_missing_command_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(&path, "- prompt: \"Password:\"\n").unwrap();
        let err = load(&path, "alice").unwrap_err();
        assert!(matches!(err, AppError::InvalidEntry { .. }));
    }

    #[test]
    fn entry_with_empty_command_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(&path, "- command: []\n  prompt: \"Password:\"\n").unwrap();
        let err = load(&path, "alice").unwrap_err();
        assert!(matches!(err, AppError::InvalidEntry { .. }));
    }

    #[test]
    fn entry_missing_prompt_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(&path, "- command: [\"ssh\"]\n").unwrap();
        let err = load(&path, "alice").unwrap_err();
        assert!(matches!(err, AppError::InvalidEntry { .. }));
    }

    #[test]
    fn substitute_user_replaces_placeholder() {
        assert_eq!(
            substitute_user("{USER}@host:", "alice").unwrap(),
            "alice@host:"
        );
    }

    #[test]
    fn substitute_user_unescapes_doubled_braces() {
        assert_eq!(
            substitute_user("password.{{1,3}}for {USER}:", "alice").unwrap(),
            "password.{1,3}for alice:"
        );
    }

    #[test]
    fn substitute_user_rejects_unknown_placeholder() {
        assert!(substitute_user("{HOST}:", "alice").is_err());
    }

    #[test]
    fn substitute_user_rejects_unmatched_brace() {
        assert!(substitute_user("password.{1,3}for :", "alice").is_err());
    }

    #[test]
    fn command_tokens_are_never_substituted() {
        // Substitution only ever applies to `prompt`, never to `command`
        // tokens.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        fs::write(
            &path,
            "- command: [\"echo\", \"{USER}\"]\n  prompt: \"Password:\"\n",
        )
        .unwrap();
        let prompts = load(&path, "alice").unwrap();
        assert_eq!(prompts[0].command, vec!["echo", "{USER}"]);
    }

    #[test]
    fn find_prompt_matches_the_longest_prefix() {
        let prompts = vec![
            prompt(&["ssh"], "generic ssh prompt", DEFAULT_NAME),
            prompt(&["ssh", "dev-server"], "dev-server prompt", "work"),
            prompt(&["sudo"], "sudo prompt", DEFAULT_NAME),
        ];

        assert_eq!(
            find_prompt(&toks(&["ssh", "dev-server"]), &prompts).map(|p| p.prompt.as_str()),
            Some("dev-server prompt")
        );
        assert_eq!(
            find_prompt(&toks(&["ssh", "dev-server"]), &prompts).map(|p| p.password.as_str()),
            Some("work")
        );
        assert_eq!(
            find_prompt(&toks(&["ssh", "other-host"]), &prompts).map(|p| p.prompt.as_str()),
            Some("generic ssh prompt")
        );
        assert_eq!(find_prompt(&toks(&["sshfs", "dev-server"]), &prompts), None);
        assert_eq!(
            find_prompt(&toks(&["ssh", "-p", "2222", "dev-server"]), &prompts)
                .map(|p| p.prompt.as_str()),
            Some("generic ssh prompt")
        );
        assert_eq!(
            find_prompt(&toks(&["ssh", "dev-server", "-v"]), &prompts).map(|p| p.prompt.as_str()),
            Some("dev-server prompt")
        );
    }

    #[test]
    fn find_prompt_treats_a_token_with_spaces_as_one_element() {
        let prompts = vec![prompt(
            &["some_cmd", "an example"],
            "Password:",
            DEFAULT_NAME,
        )];
        assert_eq!(
            find_prompt(&toks(&["some_cmd", "an example"]), &prompts).map(|p| p.prompt.as_str()),
            Some("Password:")
        );
        assert_eq!(
            find_prompt(&toks(&["some_cmd", "an", "example"]), &prompts),
            None
        );
    }

    #[test]
    fn cmd_add_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        cmd_add(
            &path,
            &toks(&["ssh", "dev-server"]),
            "{USER}@dev-server's password:",
            "work",
            "alice",
        )
        .unwrap();

        let prompts = load(&path, "alice").unwrap();
        assert_eq!(
            prompts,
            vec![prompt(
                &["ssh", "dev-server"],
                "alice@dev-server's password:",
                "work"
            )]
        );
    }

    #[test]
    fn cmd_add_omits_the_password_key_for_the_default_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        cmd_add(&path, &toks(&["sudo"]), "Password:", DEFAULT_NAME, "alice").unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("password"));
    }

    #[test]
    fn cmd_add_rejects_a_duplicate_command_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        cmd_add(&path, &toks(&["sudo"]), "Password:", DEFAULT_NAME, "alice").unwrap();
        let err = cmd_add(&path, &toks(&["sudo"]), "Other:", DEFAULT_NAME, "alice").unwrap_err();
        assert!(matches!(err, AppError::PromptEntryExists { .. }));
    }

    #[test]
    fn cmd_add_rejects_an_invalid_regex_template() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        let err = cmd_add(&path, &toks(&["sudo"]), "{HOST}:", DEFAULT_NAME, "alice").unwrap_err();
        assert!(matches!(err, AppError::InvalidPlaceholder { .. }));
    }

    #[test]
    fn cmd_remove_deletes_the_exact_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        cmd_add(&path, &toks(&["sudo"]), "Password:", DEFAULT_NAME, "alice").unwrap();
        cmd_remove(&path, &toks(&["sudo"])).unwrap();

        let prompts = load(&path, "alice").unwrap();
        assert!(prompts.is_empty());
    }

    #[test]
    fn cmd_remove_errors_when_nothing_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompts.yml");
        let err = cmd_remove(&path, &toks(&["sudo"])).unwrap_err();
        assert!(matches!(err, AppError::PromptEntryNotFound { .. }));
    }
}
