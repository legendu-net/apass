//! Command-line interface. Mirrors `apass.py::parse_args` (`apass.py:103-123`).

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "apass",
    about = "A tool to run commands without entering your password repeatedly.",
    after_help = "Options of the command to run are passed through as-is, e.g., apass some_cmd \
                  --opt1 val1 --opt2.\nPrefix the command with -- if it would otherwise be parsed \
                  as an option of apass."
)]
pub struct Cli {
    /// The command to execute.
    #[arg(trailing_var_arg = true, required = true, num_args = 1..)]
    pub command: Vec<String>,
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
    fn no_command_is_an_error() {
        let err = Cli::try_parse_from(["apass"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn a_bare_double_dash_with_nothing_after_it_is_also_an_error() {
        // Matches apass.py: argparse's REMAINDER captures `["--"]`, the
        // leading `--` is stripped, and an empty command is rejected.
        let err = Cli::try_parse_from(["apass", "--"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn options_of_the_wrapped_command_pass_through() {
        let cli = Cli::try_parse_from(["apass", "some_cmd", "--opt1", "val1", "--opt2"]).unwrap();
        assert_eq!(cli.command, vec!["some_cmd", "--opt1", "val1", "--opt2"]);
    }

    #[test]
    fn a_leading_double_dash_is_stripped() {
        // Needed only when the wrapped command itself starts with `-`.
        let cli = Cli::try_parse_from(["apass", "--", "-weird-cmd", "-x"]).unwrap();
        assert_eq!(cli.command, vec!["-weird-cmd", "-x"]);
    }

    #[test]
    fn hyphen_options_past_the_first_token_pass_through_untouched() {
        // apass2/README.md: `apass ssh -h` runs `ssh -h`, it does not show
        // apass's own help.
        let cli = Cli::try_parse_from(["apass", "ssh", "-h"]).unwrap();
        assert_eq!(cli.command, vec!["ssh", "-h"]);
    }
}
