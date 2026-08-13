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

/// Pre-seed a cache entry so the run under test never has to prompt an
/// interactive user for a password.
fn seed_cached_password(config_dir: &Path, password: &str) {
    let now = chrono::Local::now().naive_local();
    let json = format!(
        "{{\n    \"password\": \"{}\",\n    \"time\": \"{}\"\n}}",
        STANDARD.encode(password.as_bytes()),
        now.format("%Y-%m-%d %H:%M:%S%.6f")
    );
    fs::write(config_dir.join("profile.json"), json).unwrap();
}

#[test]
fn fills_in_the_password_and_hands_the_terminal_back() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join(".config").join("apass");
    write_prompts(&config_dir);
    seed_cached_password(&config_dir, "hunter2");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_apass"));
    cmd.env("HOME", home.path());
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
    // back through `apass`'s own stdout (mirroring `child.logfile_read` in
    // apass.py), then apass fills it in without any input from us and
    // hands control back for `sh` to print its result.
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
    seed_cached_password(&config_dir, "hunter2");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_apass"));
    cmd.env("HOME", home.path());
    cmd.args(["echo", "hello"]);

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
    seed_cached_password(&config_dir, "hunter2");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_apass"));
    cmd.env("HOME", home.path());
    cmd.args(["sh", "-c", "echo already-unlocked-output"]);

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
