//! The crate's one test harness (bmo-chat's docs/conventions.md, 「Rust builds」): each file of tests a
//! module of it, sharing the process.

mod support;

mod acts;
mod avatar;
mod config;
mod file_ids;
mod file_sends;
mod gathering;
mod lines;
mod owner;
mod picture_sends;
mod policy;
mod reading;
mod repository;
mod seam;
mod sends;
mod served;
mod supervisor_bots;
