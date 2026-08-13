# apass  |  [@GitHub](https://github.com/legendu-net/apass)

Run commands without the need to enter your password (repeatedly).

`apass` wraps a command with [expectrl](https://docs.rs/expectrl/) (the same idea as Python's
[pexpect](https://pexpect.readthedocs.io/)),
waits for a known password prompt,
sends your password,
and then hands the terminal back to you.
The password is cached locally so that you enter it only once.
See [Security](#security) — the cache does not expire and is not encrypted.

This is the Rust port of [`apass`](https://github.com/legendu-net/apass)'s Python implementation.
The two share the same config file formats, so `~/.config/apass/` can be used interchangeably
with either version.

## Installation

```bash
cargo install --path .
```

## Configuration

`apass` reads its configuration from `~/.config/apass/`.

### `~/.config/apass/prompts.yml`

A list of entries, each pairing a command with a regular expression matching the password
prompt that command emits.

```yaml
- command: ["ssh", "dev-server"]
  prompt: "{USER}@dev-server's password:"
- command: ["ssh"]
  prompt: ".*[Pp]assword:"
- command: ["sudo"]
  prompt: "\\[sudo\\] password for {USER}:"
```

`command` is a list of tokens matched against the **beginning** of the command you run,
token by token. The **longest** matching entry wins, so a specific entry can override a
general one:

```bash
apass ssh dev-server      # matches ["ssh", "dev-server"]
apass ssh other-host      # matches ["ssh"]
apass sshfs dev-server    # matches neither — tokens are compared whole
```

Tokens are matched literally, so `apass /usr/bin/sudo -i` needs the entry
`["/usr/bin/sudo"]`, not `["sudo"]`.

Options before a positional parameter break the prefix — `apass ssh -p 2222 dev-server`
matches `["ssh"]`, not `["ssh", "dev-server"]`. Everything past the matched prefix is
ignored, so `["ssh", "dev-server"]` still matches `apass ssh dev-server -v`.

A token containing spaces is one list element, with no quoting rules of its own:

```yaml
- command: ["some_cmd", "an example"]
  prompt: "Password:"
```

matches `apass some_cmd "an example"`, and does not match `apass some_cmd an example`.

The file is created (empty) on first run if it does not exist.

In `prompt`, the placeholder `{USER}` is substituted with the current username.
Substitution applies to `prompt` only — a `{USER}` in a `command` token is left as-is and
the match will fail.

Because `prompt` goes through the same substitution rules as Python's `str.format`, any
literal brace in a regex must be doubled. A regex quantifier such as `password.{1,3}for` has
to be written:

```yaml
- command: ["some-command"]
  prompt: "password.{{1,3}}for {USER}:"
```

### `~/.config/apass/profile.json`

Written automatically. Caches the password between runs.

## Usage

Run a configured command:

```bash
apass ssh dev-server
```

Options of the command are passed through as-is:

```bash
apass some_cmd --opt1 val1 --opt2
```

A leading `--` is also accepted, and is needed only when the command itself starts with
`-`:

```bash
apass -- some_cmd --opt1 val1 --opt2
```

A command is required — running `apass` with no arguments is an error.

## Differences from the Python version

Unlike the Python version (which leaves the spawned command's pty at a fixed 80x24), this
port sets the pty to the real size of your terminal at startup and keeps it in sync as you
resize the window — useful for `ssh` and other full-screen programs.

## Security

The cached password in `~/.config/apass/profile.json` is **base64-encoded, not encrypted**.
Base64 is trivially reversible — treat that file as if it contained the plaintext password.
Anyone who can read it can recover your password.

Restrict its permissions:

```bash
chmod 600 ~/.config/apass/profile.json
```

The cache currently never expires, so the password persists on disk until the file is
deleted:

```bash
rm ~/.config/apass/profile.json
```
