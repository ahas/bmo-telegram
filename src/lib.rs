//! The Telegram chat provider, `bmo-telegram` (docs/boundary-api.md): the bot account's connection to the Bot
//! API, the provider's declaration, the send verbs and the `telegram-html` dialect, served as bmo-chat's provider
//! routes. The owner's account is the reader's, in `reader/`, reached over the seam on the loopback.

pub mod acts;
pub mod albums;
pub mod bot_api;
pub mod compose;
pub mod files;
pub mod gather;
pub mod kinds;
pub mod owner;
pub mod pictures;
pub mod policy;
pub mod provider;
pub mod reader;
pub mod reading;
pub mod receiver;
pub mod seam;
pub mod seen;
pub mod send;
pub mod telegram;
pub mod wire;
