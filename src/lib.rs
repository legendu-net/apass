//! `apass`: run a command without the need to enter its password
//! (repeatedly), using `expectrl` to drive a pty.

pub mod cli;
pub mod config;
pub mod error;
pub mod password;
pub mod prompts;
pub mod run;

pub use error::AppError;
