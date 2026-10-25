//! Tidyward as a library: the whole pipeline behind the `tidyward` command, without any
//! printing or prompting, so apps can drive it too.
//!
//! - [`config`]: the data folder, `config.toml`, `rules.txt`, the provider key
//! - [`scan`]: scan → identify → rules → plan, with the file memory
//! - [`apply`]: carry out chosen plan items one at a time, and undo them
//!
//! Callers decide what to confirm; nothing here moves a file the caller did not pass in.

pub mod apply;
pub mod config;
pub mod scan;

pub use apply::{Outcome, apply_item, undo};
pub use config::{Config, Paths};
pub use scan::{Plan, PlanItem, PlanRule, ScanResult, Totals};
