//! Drives the compiled `apass` binary under a real pty and checks that it
//! fills in a password prompt for a spawned command and hands the terminal
//! back, end to end.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use expectrl::session::OsSession;
use expectrl::{Eof, Expect, Regex};

fn write_prompts(config_dir: &Path) {
    fs::create_dir_all(config_dir).unwrap();
    fs::write(
        config_dir.join("prompts.yml"),
        "- command: [\"sh\"]\n  prompt: \"PWPROMPT:\"\n",
    )
    .unwrap();
}

/// Like [`write_prompts`], but the entry names a non-default password.
fn write_prompts_with_named_password(config_dir: &Path, password_name: &str) {
    fs::create_dir_all(config_dir).unwrap();
    fs::write(
        config_dir.join("prompts.yml"),
        format!("- command: [\"sh\"]\n  prompt: \"PWPROMPT:\"\n  password: {password_name}\n"),
    )
    .unwrap();
}

/// Pre-seed a cache entry so the run under test never has to prompt an
/// interactive user for a password.
fn seed_cached_password(config_dir: &Path, name: &str, password: &str) {
    let now = chrono::Local::now().naive_local();
    let json = format!(
        "{{\n  \"{}\": {{\"password\": \"{}\", \"time\": \"{}\"}}\n}}",
        name,
        STANDARD.encode(password.as_bytes()),
        now.format("%Y-%m-%d %H:%M:%S%.6f")
    );
    fs::write(config_dir.join("profile.json"), json).unwrap();
}

fn apass_cmd(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_apass"));
    cmd.env("HOME", home);
    cmd.env("LOGNAME", "testuser");
    cmd.env("USER", "testuser");
    cmd
}

#[test]
fn fills_in_the_password_and_hands_the_terminal_back() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config").join("apass");
    write_prompts(&config_dir);
    seed_cached_password(&config_dir, "default", "hunter2");

    let mut cmd = apass_cmd(home.path());
    // Both the preamble and the prompt text are passed via environment
    // variables rather than embedded literally in the `-c` script, so
    // neither string appears in apass's own "Auto filling password for the
    // command ..." banner (which echoes the script's *source*, i.e. the
    // "$PREAMBLE_TEXT"/"$PW_PROMPT_TEXT" references, not their values).
    // That keeps the duplication check below honest: a match can only come
    // from the script actually running, not from apass quoting its argv.
    cmd.env("PREAMBLE_TEXT", "PREAMBLE_MARKER\n");
    cmd.env("PW_PROMPT_TEXT", "PWPROMPT: ");
    cmd.args([
        "run",
        "sh",
        "-c",
        // Output *before* the prompt matters here: `expect()`'s
        // `Captures::before()` covers only this preamble, while
        // `Captures::as_bytes()` covers the preamble *and* the matched
        // prompt together. Writing both (a bug caught in review) would
        // duplicate the preamble specifically -- a script whose prompt is
        // the very first output wouldn't expose that, since `before()`
        // would be empty and there'd be nothing visible to double.
        "printf \"$PREAMBLE_TEXT\"; printf \"$PW_PROMPT_TEXT\"; read -r p; echo; echo \"got:$p\"",
    ]);

    let mut session = OsSession::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(10)));

    // The target `sh` process's output up to its password prompt is echoed
    // back through `apass`'s own stdout, then apass fills it in without any
    // input from us and hands control back for `sh` to print its result.
    let caps = session.expect(Eof).unwrap();
    let transcript = String::from_utf8_lossy(caps.as_bytes());
    assert_eq!(
        transcript.matches("PREAMBLE_MARKER").count(),
        1,
        "output before the prompt should be echoed exactly once, got: {transcript:?}"
    );
    assert!(
        transcript.contains("got:hunter2"),
        "expected the command's own output in the transcript, got: {transcript:?}"
    );
}

#[test]
fn errors_when_no_prompt_matches_the_command() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config").join("apass");
    write_prompts(&config_dir);
    seed_cached_password(&config_dir, "default", "hunter2");

    let mut cmd = apass_cmd(home.path());
    cmd.args(["run", "echo", "hello"]);

    let mut session = OsSession::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(10)));
    session
        .expect(Regex("No prompt whose command is a prefix"))
        .unwrap();
}

#[test]
fn succeeds_when_the_command_exits_without_ever_prompting() {
    // Mirrors a tool like `gopass` that manages its own credential cache:
    // when it's already unlocked from an earlier run, it never prints its
    // password prompt at all, and apass shouldn't treat that as a failure.
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config").join("apass");
    write_prompts(&config_dir);
    seed_cached_password(&config_dir, "default", "hunter2");

    let mut cmd = apass_cmd(home.path());
    cmd.args(["run", "sh", "-c", "echo already-unlocked-output"]);

    let mut session = OsSession::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(10)));

    let caps = session.expect(Eof).unwrap();
    let transcript = String::from_utf8_lossy(caps.as_bytes());
    assert!(
        transcript.contains("already-unlocked-output"),
        "expected the command's own output to still be shown, got: {transcript:?}"
    );
    assert!(
        !transcript.contains("EOF was reached"),
        "the raw expectrl EOF error should not leak to the user, got: {transcript:?}"
    );
}

#[test]
fn uses_the_password_named_by_the_matching_prompt_entry() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config").join("apass");
    write_prompts_with_named_password(&config_dir, "work");
    seed_cached_password(&config_dir, "work", "correct-horse");

    let mut cmd = apass_cmd(home.path());
    cmd.args([
        "run",
        "sh",
        "-c",
        "printf 'PWPROMPT: '; read -r p; echo; echo got:$p",
    ]);

    let mut session = OsSession::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(10)));
    let caps = session.expect(Eof).unwrap();
    let transcript = String::from_utf8_lossy(caps.as_bytes());
    assert!(
        transcript.contains("got:correct-horse"),
        "expected the 'work' password to be used, got: {transcript:?}"
    );
}

#[test]
fn errors_when_the_named_password_is_not_cached() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config").join("apass");
    write_prompts_with_named_password(&config_dir, "work");
    // No `profile.json` seeded at all -- "work" has never been cached.

    let mut cmd = apass_cmd(home.path());
    cmd.args(["run", "sh", "-c", "printf 'PWPROMPT: '"]);

    let mut session = OsSession::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(10)));
    session
        .expect(Regex("no password named \"work\" is cached"))
        .unwrap();
}

#[test]
fn password_list_and_remove_work_through_the_compiled_binary() {
    // Unlike `password set` (which reads from `/dev/tty` via `rpassword` and
    // so needs a real pty), `list`/`remove` never touch stdin, so a plain
    // `Command::output()` is enough to exercise the CLI-parsing ->
    // subcommand-dispatch path end to end.
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config").join("apass");
    fs::create_dir_all(&config_dir).unwrap();
    seed_cached_password(&config_dir, "work", "correct-horse");

    let list = |home: &Path| {
        String::from_utf8(
            apass_cmd(home)
                .args(["passwd", "list"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
    };

    assert!(list(home.path()).contains("work"));

    let remove = apass_cmd(home.path())
        .args(["passwd", "remove", "work"])
        .output()
        .unwrap();
    assert!(remove.status.success());
    assert!(!list(home.path()).contains("work"));

    // Removing an already-absent name is an error, surfaced on stderr.
    let remove_again = apass_cmd(home.path())
        .args(["passwd", "remove", "work"])
        .output()
        .unwrap();
    assert!(!remove_again.status.success());
    assert!(String::from_utf8_lossy(&remove_again.stderr).contains("no password named \"work\""));
}

#[test]
fn prompt_add_list_and_remove_work_through_the_compiled_binary() {
    let home = tempfile::tempdir().unwrap();

    let add = apass_cmd(home.path())
        .args([
            "prompt",
            "add",
            "--regex",
            "Password:",
            "--password",
            "work",
            "--",
            "ssh",
            "dev-server",
        ])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&add.stderr)
    );

    let list = apass_cmd(home.path())
        .args(["prompt", "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("ssh dev-server"), "got: {stdout:?}");
    assert!(stdout.contains("work"), "got: {stdout:?}");

    let remove = apass_cmd(home.path())
        .args(["prompt", "remove", "--", "ssh", "dev-server"])
        .output()
        .unwrap();
    assert!(remove.status.success());

    let list_after = apass_cmd(home.path())
        .args(["prompt", "list"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&list_after.stdout)
            .trim()
            .is_empty()
    );
}

#[test]
fn h_is_an_alias_for_the_auto_generated_help_subcommand() {
    // `h` is patched onto clap's auto-generated `help` subcommand at
    // runtime (`Cli::parse()`, not a derive attribute -- see `cli.rs`), so
    // unlike `r`/`pw`/`p` it can't be exercised via `Cli::try_parse_from`
    // and needs to go through the compiled binary.
    let home = tempfile::tempdir().unwrap();

    let via_alias = apass_cmd(home.path()).arg("h").output().unwrap();
    let via_full_name = apass_cmd(home.path()).arg("help").output().unwrap();

    assert!(via_alias.status.success());
    assert_eq!(via_alias.stdout, via_full_name.stdout);
}
