//! Regression test for a crash found during review: `expectrl`'s `polling`
//! backend calls `epoll_wait`, which does not auto-restart when a signal is
//! caught (`SIGWINCH` here, used to keep the child pty's size in sync with
//! the real terminal -- see `run::interact`). Without the retry in
//! `run::interact`, resizing the terminal mid-session made `apass::spawn`
//! return an `Interrupted` IO error, which killed `apass` -- and, via its
//! `PtyProcess`'s `Drop`, the command it was wrapping (e.g. a live `ssh`
//! session).

use std::fs;
use std::process::Command;
use std::time::Duration;

use expectrl::session::OsSession;
use expectrl::{Expect, Regex};

fn write_config(home: &std::path::Path) {
    let config_dir = home.join(".config").join("apass");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("prompts.yml"),
        "- command: [\"sh\"]\n  prompt: \"Password:\"\n",
    )
    .unwrap();
    let now = chrono::Local::now().naive_local();
    let json = format!(
        "{{\n  \"default\": {{\"password\": \"{}\", \"time\": \"{}\"}}\n}}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"hunter2"),
        now.format("%Y-%m-%d %H:%M:%S%.6f")
    );
    fs::write(config_dir.join("profile.json"), json).unwrap();
}

fn spawn_apass_wrapping_a_slow_command(home: &std::path::Path) -> OsSession {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_apass"));
    cmd.env("HOME", home);
    cmd.env("LOGNAME", "testuser");
    cmd.env("USER", "testuser");
    cmd.args([
        "run",
        "sh",
        "-c",
        "printf 'Password: '; read -r p; echo; sleep 5; echo done:$p",
    ]);
    let mut session = OsSession::spawn(cmd).unwrap();
    session.set_expect_timeout(Some(Duration::from_secs(10)));
    session.expect(Regex("Password:")).unwrap();
    session
}

#[test]
fn survives_a_terminal_resize_mid_session() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path());
    let mut session = spawn_apass_wrapping_a_slow_command(home.path());

    // Generous margin over apass's own internal 300ms pre-send delay
    // (run::run) before it registers the SIGWINCH handler and enters
    // interact() -- SIGWINCH's default disposition is "ignore", so a
    // signal delivered before registration is silently dropped rather than
    // causing a false failure, but it would make this test pass vacuously
    // (never exercising the regression it's meant to catch) on a slow or
    // contended machine with a tighter margin.
    std::thread::sleep(Duration::from_secs(1));
    let pid = session.get_process().pid();
    unsafe {
        libc::kill(pid.as_raw(), libc::SIGWINCH);
    }

    // Before the fix, apass (and the `sh` command it wraps) would already
    // be dead at this point, and this would fail with `Err(Eof)`.
    session.expect(Regex("done:hunter2")).unwrap();
}

#[test]
fn survives_rapid_repeated_resizes() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path());
    let mut session = spawn_apass_wrapping_a_slow_command(home.path());

    std::thread::sleep(Duration::from_secs(1));
    let pid = session.get_process().pid();
    // Spread across a full second, not just the first 200ms, so at least
    // one signal is virtually guaranteed to land after apass has
    // registered its handler and entered interact(), even under load.
    for _ in 0..40 {
        unsafe {
            libc::kill(pid.as_raw(), libc::SIGWINCH);
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    session.expect(Regex("done:hunter2")).unwrap();
}
