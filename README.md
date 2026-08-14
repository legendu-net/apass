# apass  |  [@GitHub](https://github.com/legendu-net/apass)

Run commands without the need to enter your password (repeatedly).

`apass` wraps a command with [expectrl](https://docs.rs/expectrl/),
waits for a known password prompt,
sends your password,
and then hands the terminal back to you.
Passwords are cached locally, by name, so you enter each one only once.
See [Security](#security) — the cache does not expire and is not encrypted.

## Installation

```bash
cargo install --path .
```

## Usage

`apass` has three subcommands: `run` (wrap a command), `passwd` (manage cached passwords), and
`prompt` (manage `prompts.yml` entries). Each has a short alias — `r`, `pw`, `p` — and `help` (also
built in) has the alias `h`.

### `apass run`

```bash
apass run ssh dev-server
```

Options of the command are passed through as-is:

```bash
apass run some_cmd --opt1 val1 --opt2
```

A leading `--` is also accepted, and is needed only when the command itself starts with `-`:

```bash
apass run -- some_cmd --opt1 val1 --opt2
```

A command is required — `apass run` with no arguments is an error.

If the matching `prompts.yml` entry names a password that isn't cached yet, `apass run` fails with
an error telling you to run `apass passwd set <name>` first.

### `apass passwd`

```bash
apass passwd set [<name>]     # prompt (no echo) and cache a password; name defaults to "default"
apass passwd list             # names + when each was cached; never prints a secret
apass passwd remove <name>
```

### `apass prompt`

```bash
apass prompt list
apass prompt add --regex <regex> [--password <name>] [--] <command>...
apass prompt remove [--] <command>...
apass prompt edit               # opens prompts.yml in $VISUAL, then $EDITOR, then vi
```

`prompt add` and `prompt remove` rewrite the whole `prompts.yml` file, which drops any comments and
reflows formatting. Use `apass prompt edit` instead if you keep comments in the file — it opens your
editor and validates the file once you save.

## Configuration

`apass` reads its configuration from `~/.config/apass/`.

### `~/.config/apass/prompts.yml`

A list of entries, each pairing a command with a regular expression matching the password prompt
that command emits, and (optionally) the name of the password to send.

```yaml
- command: ["ssh", "dev-server"]
  prompt: "{USER}@dev-server's password:"
  password: work
- command: ["ssh"]
  prompt: ".*[Pp]assword:"
- command: ["sudo"]
  prompt: "\\[sudo\\] password for {USER}:"
```

`password` is optional and defaults to `"default"`.

`command` is a list of tokens matched against the **beginning** of the command you run,
token by token. The **longest** matching entry wins, so a specific entry can override a
general one:

```bash
apass run ssh dev-server      # matches ["ssh", "dev-server"]
apass run ssh other-host      # matches ["ssh"]
apass run sshfs dev-server    # matches neither — tokens are compared whole
```

Tokens are matched literally, so `apass run /usr/bin/sudo -i` needs the entry
`["/usr/bin/sudo"]`, not `["sudo"]`.

Options before a positional parameter break the prefix — `apass run ssh -p 2222 dev-server`
matches `["ssh"]`, not `["ssh", "dev-server"]`. Everything past the matched prefix is
ignored, so `["ssh", "dev-server"]` still matches `apass run ssh dev-server -v`.

A token containing spaces is one list element, with no quoting rules of its own:

```yaml
- command: ["some_cmd", "an example"]
  prompt: "Password:"
```

matches `apass run some_cmd "an example"`, and does not match `apass run some_cmd an example`.

The file is created (empty) on first run if it does not exist.

In `prompt`, the placeholder `{USER}` is substituted with the current username.
Substitution applies to `prompt` only — a `{USER}` in a `command` token or in `password` is left
as-is.

Because `prompt` goes through `{USER}`-style placeholder substitution, any literal brace in a regex
must be doubled. A regex quantifier such as `password.{1,3}for` has to be written:

```yaml
- command: ["some-command"]
  prompt: "password.{{1,3}}for {USER}:"
```

### `~/.config/apass/profile.json`

Written automatically by `apass passwd set`. Caches passwords by name:

```json
{
  "default": { "password": "...", "time": "..." },
  "work":    { "password": "...", "time": "..." }
}
```

## Terminal resizing

`apass` sets the spawned command's pty to the real size of your terminal at startup and keeps it in
sync as you resize the window — useful for `ssh` and other full-screen programs.

## Security

Cached passwords in `~/.config/apass/profile.json` are **base64-encoded, not encrypted**.
Base64 is trivially reversible — treat that file as if it contained plaintext passwords.
Anyone who can read it can recover them.

Restrict its permissions:

```bash
chmod 600 ~/.config/apass/profile.json
```

Cached passwords currently never expire, so they persist on disk until removed:

```bash
apass passwd remove <name>
```
