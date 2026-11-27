//! The server's integration tests, in one binary: linked once, the tests of every module run in
//! parallel.

mod common;

mod ai;
mod api;
mod apply;
mod attachments;
mod local;
mod share;
mod snapshot;
mod sync;
mod tools;
mod transfer;
mod upgrade;
mod web;
