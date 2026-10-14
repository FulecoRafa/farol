//! The built-in plugins. Each is a [`crate::engine::Provider`]; external
//! ones speak the same rows over a socket ([`external`]).

pub mod apps;
pub mod calc;
pub mod clipboard;
pub mod emoji;
pub mod farol;
pub mod files;
pub mod timers;
pub mod web;
pub mod windows;
