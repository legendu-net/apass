//! Load and query `prompts.yml`.
//!
//! Mirrors `apass.py::_get_prompts` (`apass.py:26-53`) and
//! `apass.py::_find_prompt` (`apass.py:56-66`).

use std::fs;
use std::path::Path;

use serde_yaml_ng::Value;

use crate::error::AppError;

/// A single configured prompt: the command prefix it matches, and the
/// (already `{USER}`-substituted) regex for the password prompt that
/// command emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub command: Vec<String>,
    pub prompt: String,
}

/// Load `prompts.yml`, creating an empty one if it doesn't exist yet.
pub fn load(path: &Path, user: &str) -> Result<Vec<Prompt>, AppError> {
    if !path.is_file() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_yaml_ng::to_string(&Vec::<Value>::new())?)?;
    }

    let text = fs::read_to_string(path)?;
    let value: Value = serde_yaml_ng::from_str(&text)?;
    // An empty file parses as `Null`; treat that the same as an empty list,
    // just as `yaml.safe_load(f) or []` does in the Python version.
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
    Some(Prompt { command, prompt })
}

fn format_entry(item: &Value) -> String {
    serde_yaml_ng::to_string(item)
        .map(|s| s.trim().replace('\n', " "))
        .unwrap_or_else(|_| format!("{item:?}"))
}

/// Emulates the subset of Python's `str.format(USER=...)` used by
/// `prompts.yml`: `{USER}` is substituted, `{{`/`}}` become literal braces,
/// and any other placeholder (or an unmatched brace) is an error.
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
pub fn find_prompt<'a>(command: &[String], prompts: &'a [Prompt]) -> Option<&'a str> {
    let mut best_length = 0;
    let mut best_prompt = None;
    for p in prompts {
        if p.command.len() > best_length
            && command.get(..p.command.len()) == Some(p.command.as_slice())
        {
            best_length = p.command.len();
            best_prompt = Some(p.prompt.as_str());
        }
    }
    best_prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|t| t.to_string()).collect()
    }

    fn prompt(command: &[&str], text: &str) -> Prompt {
        Prompt {
            command: toks(command),
            prompt: text.to_string(),
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
                prompt(&["ssh", "dev-server"], "alice@dev-server's password:"),
                prompt(&["sudo"], "[sudo] password for alice:"),
            ]
        );
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
        // A `{USER}` in a command token is left as-is by the Python version
        // too -- substitution only ever applies to `prompt`.
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
            prompt(&["ssh"], "generic ssh prompt"),
            prompt(&["ssh", "dev-server"], "dev-server prompt"),
            prompt(&["sudo"], "sudo prompt"),
        ];

        assert_eq!(
            find_prompt(&toks(&["ssh", "dev-server"]), &prompts),
            Some("dev-server prompt")
        );
        assert_eq!(
            find_prompt(&toks(&["ssh", "other-host"]), &prompts),
            Some("generic ssh prompt")
        );
        assert_eq!(find_prompt(&toks(&["sshfs", "dev-server"]), &prompts), None);
        assert_eq!(
            find_prompt(&toks(&["ssh", "-p", "2222", "dev-server"]), &prompts),
            Some("generic ssh prompt")
        );
        assert_eq!(
            find_prompt(&toks(&["ssh", "dev-server", "-v"]), &prompts),
            Some("dev-server prompt")
        );
    }

    #[test]
    fn find_prompt_treats_a_token_with_spaces_as_one_element() {
        let prompts = vec![prompt(&["some_cmd", "an example"], "Password:")];
        assert_eq!(
            find_prompt(&toks(&["some_cmd", "an example"]), &prompts),
            Some("Password:")
        );
        assert_eq!(
            find_prompt(&toks(&["some_cmd", "an", "example"]), &prompts),
            None
        );
    }
}
