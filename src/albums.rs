//! The albums the bot account has read lately. Telegram names the messages of one album by a media group of
//! its own, and the chat protocol names them by the album's first message (bmo-chat's `docs/contract.md`,
//! 「Messages」), so each group is held to the first of its messages read. The messages of one album arrive
//! together, so only the groups read last are remembered.

use std::collections::VecDeque;

/// How many groups are remembered.
pub const KEPT: usize = 64;

#[derive(Default)]
pub struct AlbumNames {
  // Each group as the room and the platform's name for it, with the first message read of it.
  firsts: VecDeque<(i64, String, i64)>,
}

impl AlbumNames {
  /// The album a message of the group went out in.
  pub fn name(&mut self, chat_id: i64, group: &str, message_id: i64) -> i64 {
    if let Some((_, _, first)) =
      self.firsts.iter().find(|(chat, named, _)| *chat == chat_id && named == group)
    {
      return *first;
    }

    self.firsts.push_back((chat_id, group.to_owned(), message_id));

    if self.firsts.len() > KEPT {
      self.firsts.pop_front();
    }

    message_id
  }
}
