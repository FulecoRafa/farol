//! The built-in plugins. Each is a [`crate::engine::Provider`]; external
//! ones speak the same rows over a socket ([`external`]).

pub mod apps;
pub mod calc;
pub mod emoji;
pub mod farol;
