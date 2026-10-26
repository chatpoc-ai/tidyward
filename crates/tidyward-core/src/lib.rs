//! Tidyward core: everything that decides what is sent to the model and what may be done to a
//! file. No network and no disk writes here, so all of it can be unit-tested and audited.
//!
//! Pipeline (see docs/design.md):
//!   identify  local signals first ([`identify::local_identify`]), the model for the rest
//!   rules     exact conditions locally ([`rules::exact_ok`]), the model only for semantic ones
//!   decide    first rule that applies wins; weak or no match is left to the user ([`engine::decide`])

pub mod engine;
pub mod identify;
pub mod item;
pub mod local;
pub mod mask;
pub mod rules;
pub mod state;
pub mod words;

pub use engine::{Decision, DecidedBy};
pub use identify::{Identity, Purpose};
pub use item::{Content, Item, Kind};
pub use rules::{Action, Rule};
