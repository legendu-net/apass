use std::time::Duration;

use clap::Parser;

use apass::cli::Cli;
use apass::config::{Config, current_username};
use apass::{AppError, password, prompts, run};

fn main() {
    if let Err(err) = try_main() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<(), AppError> {
    let cli = Cli::parse();
    let config = Config::from_home()?;
    let user = current_username()?;

    // Same order as `apass.py::main` (apass.py:126-128): the password is
    // fetched -- prompting interactively on a cache miss -- *before*
    // `prompts.yml` is loaded, so a malformed or non-matching prompts file
    // is only reported after the user has already entered their password.
    let password = password::get(
        &config.profile_path(),
        Duration::MAX,
        "Please enter your password: ",
    )?;

    let prompts_path = config.prompts_path();
    let prompts = prompts::load(&prompts_path, &user)?;

    run::run(&cli.command, &password, &prompts, &prompts_path)
}
