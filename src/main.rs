use std::time::Duration;

use apass::cli::{Cli, Command, PasswordAction, PromptAction};
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

    match cli.command {
        Command::Run(args) => run_command(&config, &args.command),
        Command::Password { action } => password_command(&config, action),
        Command::Prompt { action } => prompt_command(&config, action),
    }
}

fn run_command(config: &Config, command: &[String]) -> Result<(), AppError> {
    let user = current_username()?;
    let prompts_path = config.prompts_path();
    let prompts = prompts::load(&prompts_path, &user)?;
    let prompt =
        prompts::find_prompt(command, &prompts).ok_or_else(|| AppError::NoPromptFound {
            command: run::shell_join(command),
            path: prompts_path.clone(),
        })?;

    let profile_path = config.profile_path();
    let profile = password::Profile::load(&profile_path)?;
    let cached = profile.password(&prompt.password, Duration::MAX)?;
    let Some(password) = cached else {
        return Err(AppError::PasswordNotFound {
            name: prompt.password.clone(),
            path: profile_path,
        });
    };

    run::run(command, &password, &prompt.prompt)
}

fn password_command(config: &Config, action: PasswordAction) -> Result<(), AppError> {
    let profile_path = config.profile_path();
    match action {
        PasswordAction::Set { name } => password::cmd_set(&profile_path, &name),
        PasswordAction::List => password::cmd_list(&profile_path),
        PasswordAction::Remove { name } => password::cmd_remove(&profile_path, &name),
    }
}

fn prompt_command(config: &Config, action: PromptAction) -> Result<(), AppError> {
    let prompts_path = config.prompts_path();
    match action {
        PromptAction::List => prompts::cmd_list(&prompts_path),
        PromptAction::Add {
            regex,
            password,
            command,
        } => {
            let user = current_username()?;
            prompts::cmd_add(&prompts_path, &command, &regex, &password, &user)
        }
        PromptAction::Remove { command } => prompts::cmd_remove(&prompts_path, &command),
        PromptAction::Edit => {
            let user = current_username()?;
            prompts::cmd_edit(&prompts_path, &user)
        }
    }
}
