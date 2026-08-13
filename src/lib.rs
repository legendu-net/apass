//! `apass`: run a command without the need to enter its password
//! (repeatedly). A Rust port of `apass.py`, using `expectrl` in place of
//! `pexpect`.

pub mod cli;
pub mod config;
pub mod error;
pub mod password;
pub mod prompts;
pub mod run;

pub use error::AppError;
