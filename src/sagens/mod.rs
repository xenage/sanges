mod app;
mod args;
mod client;
pub(crate) mod config;
pub(crate) mod daemon;
mod output;
mod recovery;
pub(crate) mod ui;

pub use app::run;
