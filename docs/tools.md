# The hosted tool

The provider's one hosted tool, `bmo/telegram_messages`: one chat's recent messages read through the owner's
account without a room standing for it. It is placed by the deployment and registered with the tool host, the
way the receiver's rooms tools are; the manifest stands in `tools/messages/`, with the manual and the input
schema beside it.

- **The call** names `chat` (a number, or the name or handle the owner's account shows), `max` (how many
  messages at most, fifty where absent and two hundred the most) and `since` (the earliest moment in epoch
  milliseconds, or none). The answer is newest first, each line the moment, the sender's name and number, and
  the words as they are; a picture, file, sound or other non-text line carries its kind's placeholder.
- **A name that means exactly one chat** is read at once. Where a name could mean several chats or contacts,
  the provider answers the candidates by number, name and kind, so the owner picks; the next call names the
  number.
- **The owner's word alone.** The tool reads the turn's owner mark from the session record, the run a resident
  program's place names standing over its variables, and refuses a turn that is not the owner's, saying that
  no grant in force names reading a chat.
- **Nothing is sent into the chat and no room is made.** The tool reads the provider's own `GET /messages`
  route, which reads the reader's `GET /reader/messages`; the dialog list, the history walk and the record of
  what was seen are left as they stand.
- **Where the provider is reached:** the tool reads `BMO_TELEGRAM_TOKEN` and `BMO_TELEGRAM_PORT` from
  `env/telegram.env` under the data root the session record names, and calls the service on the loopback. It
  holds no token of its own and reads no other unit's files.
