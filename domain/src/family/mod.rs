//! The family: one aggregate, and what is in it.
//!
//! - [`members`]: the family itself, its head, people and AI agents
//! - [`tasks`]: task lists and tasks
//! - [`places`]: place groups, places and their locations
//! - [`problems`]: problems, their solutions and steps, each person's day plan
//!
//! The rest is shared: [`commands`] and [`events`] are the vocabulary,
//! [`errors`] says why a command is refused, [`state`] is the family as
//! replayed from events, and [`aggregate`] decides commands and writes events.

pub mod aggregate;
pub mod commands;
pub mod errors;
pub mod events;
pub mod members;
pub mod places;
pub mod problems;
pub mod state;
pub mod tasks;
