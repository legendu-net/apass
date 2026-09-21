//! Spawn the target command, fill in its password prompt, and hand the
//! terminal back to the user.

use std::io::{self, Write};
use std::process::Command as StdCommand;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use expectrl::process::Termios;
use expectrl::session::OsSession;
use expectrl::stream::stdin::Stdin;
use expectrl::{Expect, Regex};
use terminal_size::{Height, Width};

use crate::error::AppError;

/// How long to wait for the configured password prompt before giving up.
const EXPECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Spawn `command`, wait for `prompt` (a regex), send `password`, and hand
/// the terminal back to the user. Resolving which prompt/password to use is
/// the caller's job -- see `main.rs`'s `run` dispatch -- since that needs
/// both `prompts.yml` and the named password store.
pub fn run(command: &[String], password: &str, prompt: &str) -> Result<(), AppError> {
    eprintln!(
        "Auto filling password for the command {}",
        shell_join(command)
    );

    let mut cmd = StdCommand::new(&command[0]);
    cmd.args(&command[1..]);
    let mut session = OsSession::spawn(cmd)?;
    session.set_expect_timeout(Some(EXPECT_TIMEOUT));
    apply_window_size(&mut session);

    // Stream everything read while waiting for the prompt back to the
    // user's terminal. `as_bytes()` already includes the text before the
    // match, so it must not also be combined with `before()` -- that would
    // print the pre-match text twice.
    match session.expect(Regex(prompt)) {
        Ok(caps) => {
            io::stdout().write_all(caps.as_bytes())?;
            io::stdout().flush()?;

            thread::sleep(Duration::from_millis(300));
            session.send_line(password)?;
        }
        // The command exited on its own before ever showing the prompt --
        // e.g. `gopass` already had a cached/unlocked session from an
        // earlier run, so this invocation never needed the password.
        // `expectrl` doesn't hand back the bytes it already buffered when
        // `expect()` fails with `Eof`, so re-`expect`ing `Eof` (which always
        // matches once the stream is at EOF) is what recovers the child's
        // output instead of losing it.
        Err(expectrl::Error::Eof) => {
            let caps = session.expect(expectrl::Eof)?;
            io::stdout().write_all(caps.as_bytes())?;
            io::stdout().flush()?;
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    }

    interact(session)
}

/// Hand the terminal to the user, keeping the child's pty in sync with the
/// real terminal size so `ssh`/full-screen programs render correctly.
fn interact(mut session: OsSession) -> Result<(), AppError> {
    let resize_pending = Arc::new(AtomicBool::new(false));
    // Best-effort: if registering the handler fails there's nothing more
    // sensible to do than continue without live resizing.
    let _ = signal_hook::flag::register(signal_hook::consts::SIGWINCH, Arc::clone(&resize_pending));

    // Enable the child pty's echo ourselves, up front, instead of letting
    // `InteractSession::spawn()` toggle it on entry/exit as it normally
    // would. Its on/off bookkeeping is local to a single `spawn()` call and
    // isn't safe across the retry loop below: a call interrupted by
    // SIGWINCH after turning echo on, but before its matching restore,
    // would make the *next* attempt see echo as already on and skip
    // restoring it -- silently leaving a later password-style prompt in
    // this same session unmasked. Doing it here once, and restoring it
    // once after the loop, sidesteps that regardless of how many retries
    // happen.
    let echo_was_on = session.is_echo().unwrap_or(true);
    if !echo_was_on {
        let _ = session.set_echo(true);
    }

    let mut stdin = Stdin::open()?;
    // `InteractSession` borrows `stdin`, so it must be dropped before
    // `stdin.close()` restores the terminal's original settings. Scoping it
    // in its own block, and keeping the error as a value instead of using
    // `?` here, lets `close()` still run when `spawn()` fails.
    let result = {
        let mut interact = session.interact(&mut stdin, io::stdout());
        interact.set_idle_action(move |ctx| {
            if resize_pending.swap(false, Ordering::Relaxed)
                && let Some((Width(cols), Height(rows))) = terminal_size::terminal_size()
            {
                let _ = ctx.session.get_process_mut().set_window_size(cols, rows);
            }
            Ok(false)
        });
        // The `polling` backend's `epoll_wait` doesn't auto-restart on a
        // caught signal (SIGWINCH here), so `spawn()` returns an
        // `Interrupted` IO error instead of just resuming -- without this
        // retry, resizing the terminal mid-session would kill `apass` and
        // the command it's wrapping (e.g. a live `ssh` session). Echo is
        // already on from above, so `spawn()`'s own toggling is a no-op on
        // every attempt.
        let r = loop {
            match interact.spawn() {
                Err(expectrl::Error::IO(io_err)) if io_err.kind() == io::ErrorKind::Interrupted => {
                    continue;
                }
                other => break other,
            }
        };
        drop(interact);
        r
    };

    if !echo_was_on {
        let _ = session.set_echo(false);
    }
    stdin.close()?;
    let is_alive = result?;
    // When the child process exits immediately after emitting its final output,
    // `interact_polling` detects `!StillAlive` at the start of its loop and
    // returns early before reading any remaining buffered data from the pty.
    // Draining up to EOF recovers that trailing output.
    if !is_alive {
        session.set_expect_timeout(Some(Duration::from_secs(1)));
        if let Ok(caps) = session.expect(expectrl::Eof) {
            io::stdout().write_all(caps.as_bytes())?;
            io::stdout().flush()?;
        }
    }
    Ok(())
}

fn apply_window_size(session: &mut OsSession) {
    if let Some((Width(cols), Height(rows))) = terminal_size::terminal_size() {
        let _ = session.get_process_mut().set_window_size(cols, rows);
    }
}

/// A minimal shell-quoting helper for display purposes only ("safe"
/// characters -- left unquoted -- are word characters plus `@%+=:,./-`).
fn shell_quote(token: &str) -> String {
    let is_safe = |c: char| c.is_ascii_alphanumeric() || "@%_+=:,./-".contains(c);
    if !token.is_empty() && token.chars().all(is_safe) {
        token.to_string()
    } else {
        format!("'{}'", token.replace('\'', r"'\''"))
    }
}

pub fn shell_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|t| shell_quote(t))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_join_leaves_safe_tokens_unquoted() {
        assert_eq!(
            shell_join(&["ssh".into(), "dev-server".into()]),
            "ssh dev-server"
        );
    }

    #[test]
    fn shell_join_quotes_tokens_with_spaces() {
        assert_eq!(
            shell_join(&["some_cmd".into(), "an example".into()]),
            "some_cmd 'an example'"
        );
    }

    #[test]
    fn shell_join_escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
