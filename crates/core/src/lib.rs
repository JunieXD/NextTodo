//! Domain types and rules shared by the server, the command line and the apps.
//!
//! Values are validated when they are deserialized, so code holding these types can trust them.

pub mod board;
pub mod dag;
pub mod model;
pub mod ops;
pub mod repeat;
pub mod sort_key;
