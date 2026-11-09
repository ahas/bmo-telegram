//! The crate's one test harness (bmo-chat's docs/conventions.md, 「Rust builds」): each file of tests a
//! module of it, sharing the process.

mod support;

mod acts;
mod config;
mod file_ids;
mod file_sends;
mod gathering;
mod owner;
mod personal_data;
mod picture_sends;
mod policy;
mod reading;
mod repository;
mod seam;
mod sends;
mod served;
