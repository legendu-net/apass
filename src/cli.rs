//! Command-line interface.

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::password::DEFAULT_NAME;

#[derive(Debug, Parser)]
#[command(
    name = "apass",
    about = "A tool to run commands without entering your password repeatedly."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Like `<Cli as clap::Parser>::parse()`, but also aliases `h` to
    /// clap's auto-generated `help` subcommand, matching the `r`/`pw`/`p`
    /// aliases below. `help` isn't a real `Command` variant -- clap
    /// synthesizes it when building the `clap::Command` -- so it can't be
    /// aliased with a derive attribute the way the others are; it has to be
    /// patched onto the built command by hand.
    pub fn parse() -> Self {
        let mut cmd = Self::command();
        cmd.build();
        let cmd = cmd.mut_subcommand("help", |c| c.visible_alias("h"));
        let mut matches = cmd.get_matches();
        Self::from_arg_matches_mut(&mut matches)
            .map_err(|e| e.format(&mut Self::command()))
            .unwrap_or_else(|e| e.exit())
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a command, auto-filling its password prompt.
    #[command(
        visible_alias = "r",
        after_help = "Options of the command to run are passed through as-is, e.g., apass \
                             run some_cmd --opt1 val1 --opt2.\nPrefix the command with -- if it \
                             would otherwise be parsed as an option of apass run."
    )]
    Run(RunArgs),

    /// Manage cached passwords.
    #[command(name = "passwd", visible_alias = "pw")]
    Password {
        #[command(subcommand)]
        action: PasswordAction,
    },

    /// Manage `prompts.yml` entries.
    #[command(visible_alias = "p")]
    Prompt {
        #[command(subcommand)]
        action: PromptAction,
    },
}

#[derive(Debug, clap::Args)]
pub struct RunArgs {
    /// The command to execute.
    #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
    pub command: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum PasswordAction {
    /// Prompt for a password (no echo) and cache it under `name`.
    Set {
        /// The name to store the password under.
        #[arg(default_value = DEFAULT_NAME)]
        name: String,
    },
    /// List cached password names and when each was cached.
    List,
    /// Remove a cached password.
    Remove {
        /// The name to remove.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum PromptAction {
    /// List configured prompt entries.
    List,
    /// Add a new prompt entry.
    Add {
        /// The regular expression matching the password prompt.
        #[arg(long)]
        regex: String,
        /// The name of the cached password to send for this entry.
        #[arg(long, default_value = DEFAULT_NAME)]
        password: String,
        /// The command prefix this entry matches.
        #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// Remove the entry whose command matches exactly.
    Remove {
        /// The command prefix to remove.
        #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// Open `prompts.yml` in `$VISUAL`/`$EDITOR`/`vi`, then validate it.
    Edit,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn help_flag_shows_help() {
        let err = Cli::try_parse_from(["apass", "-h"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn no_subcommand_is_an_error() {
        let err = Cli::try_parse_from(["apass"]).unwrap_err();
        assert_eq!(
            err.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn run_with_no_command_is_an_error() {
        let err = Cli::try_parse_from(["apass", "run"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn run_a_bare_double_dash_with_nothing_after_it_is_also_an_error() {
        let err = Cli::try_parse_from(["apass", "run", "--"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn run_options_of_the_wrapped_command_pass_through() {
        let cli =
            Cli::try_parse_from(["apass", "run", "some_cmd", "--opt1", "val1", "--opt2"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected Run");
        };
        assert_eq!(args.command, vec!["some_cmd", "--opt1", "val1", "--opt2"]);
    }

    #[test]
    fn run_a_leading_double_dash_is_stripped() {
        let cli = Cli::try_parse_from(["apass", "run", "--", "-weird-cmd", "-x"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected Run");
        };
        assert_eq!(args.command, vec!["-weird-cmd", "-x"]);
    }

    #[test]
    fn run_hyphen_options_past_the_first_token_pass_through_untouched() {
        // `apass run ssh -h` runs `ssh -h`; it does not show apass's help.
        let cli = Cli::try_parse_from(["apass", "run", "ssh", "-h"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected Run");
        };
        assert_eq!(args.command, vec!["ssh", "-h"]);
    }

    #[test]
    fn r_is_an_alias_for_run() {
        let cli = Cli::try_parse_from(["apass", "r", "ssh", "dev-server"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected Run");
        };
        assert_eq!(args.command, vec!["ssh", "dev-server"]);
    }

    #[test]
    fn pw_is_an_alias_for_passwd() {
        let cli = Cli::try_parse_from(["apass", "pw", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Password {
                action: PasswordAction::List
            }
        ));
    }

    #[test]
    fn p_is_an_alias_for_prompt() {
        let cli = Cli::try_parse_from(["apass", "p", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Prompt {
                action: PromptAction::List
            }
        ));
    }

    #[test]
    fn password_set_defaults_to_the_default_name() {
        let cli = Cli::try_parse_from(["apass", "passwd", "set"]).unwrap();
        let Command::Password {
            action: PasswordAction::Set { name },
        } = cli.command
        else {
            panic!("expected Password::Set");
        };
        assert_eq!(name, DEFAULT_NAME);
    }

    #[test]
    fn password_set_accepts_an_explicit_name() {
        let cli = Cli::try_parse_from(["apass", "passwd", "set", "work"]).unwrap();
        let Command::Password {
            action: PasswordAction::Set { name },
        } = cli.command
        else {
            panic!("expected Password::Set");
        };
        assert_eq!(name, "work");
    }

    #[test]
    fn prompt_add_parses_flags_and_trailing_command() {
        let cli = Cli::try_parse_from([
            "apass",
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
        .unwrap();
        let Command::Prompt {
            action:
                PromptAction::Add {
                    regex,
                    password,
                    command,
                },
        } = cli.command
        else {
            panic!("expected Prompt::Add");
        };
        assert_eq!(regex, "Password:");
        assert_eq!(password, "work");
        assert_eq!(command, vec!["ssh", "dev-server"]);
    }
}
