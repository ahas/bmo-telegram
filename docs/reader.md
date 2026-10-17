# The reader: the owner's account

The reader is the process in `reader/` that holds the owner's own Telegram account over MTProto. The service
starts nothing in it and reads nothing of it but its seam (`docs/boundary-api.md`, 「The seam between the
service and the reader」).

## What it is for

- **The other bots' words.** The Bot API never shows a bot what another bot says. The reader reads every new
  message of a room the receiver listens to that another bot wrote, and hands it to the service, which hands
  it to the receiver as read in the `human-reader` mode. A message the owner's account sent, one with no room,
  and one the provider's own bot wrote are not read in.
- **What the Bot API will not fetch.** A picture or a file past the Bot API's ceiling for a bot, and a
  picture fetched after the fact for a turn that answers it, are fetched through the reader.
- **The owner's own words.** A send in the owner's voice goes out in the owner's account, as a phone sends it.
- **History.** The past messages of the rooms the receiver names are walked back and handed over as history.
- **Profile pictures.** The pictures of every room and every sender the provider has seen are kept current.

## Connecting

- **One connection on the owner's account.** A session must not be used from two places at once, so the live
  reading, the history walk, the profile pictures and the owner's sends share one client, and a lock file in
  the provider's data folder keeps the session to one process. The reader opens the account only once the lock
  is its own, asking again every two seconds and saying once that it waits.
- **The session is written by the login script,** `bun run login` in `reader/`, which asks for the phone
  number, the code Telegram sends and the account's password where one is set, and prints the session to put
  in `BMO_TG_SESSION`.
- **Without its three variables the reader stands inactive,** says why on its health, and opens nothing; the
  provider then declares the `human-reader` mode inactive with that reason.

## The history walk

- **The rooms walked are the receiver's to name** (`docs/boundary-api.md`, 「Needed from the receiver」), and
  a room whose peer the account cannot resolve stores nothing rather than every chat's history.
- **Numbers.** Only a supergroup or a channel numbers its messages for everyone, so its messages keep their
  numbers. Another room's history is numbered below every number the bot account can see, older first, so a
  message read back never takes a number a live message has.
- **Progress is kept per room** in the reader's own store: the lowest number reached and whether the top of
  the room's history was reached. A walk resumes where it left off.
- **What a line out of the history says.** A picture says so, whether or not it was captioned; a message that
  is only words is handed over exactly as it was said.
- **By hand,** `bun run history <room>` in `reader/` walks one room's history; a room of a provider that
  declares no history is refused before the account is asked anything.

## Profile pictures

- **Whose picture is kept:** every room and every person the provider has seen, the bot account among them,
  each once. The owner's one-to-one with the bot wears the bot's picture, and the owner keeps their own where
  they speak. What Telegram has never heard of is left out.
- **Which picture Telegram shows** is the number of the account's photo, for a person and a room alike, and
  nothing at all where the account shows none.
- **A picture is taken again** only when nobody has taken it yet, when the picture behind it changed, or when
  the file its record points at is gone. A subject that shows none has its file taken down.
- **One pass over everyone** starts a minute after the reader connects and runs again every six hours. It
  takes what it can, counts the rest, and keeps a room and a person of one number apart. The files are kept in
  the provider's data folder, and the service serves them (`GET /avatars/{kind}/{id}`).

## Why it is TypeScript

The reader stays on the TypeScript MTProto client, `telegram` (gramjs), because no maintained Rust library
covers what it needs: a user account's login, history reading, file download and profile pictures.

- `grammers`, the one pure-Rust MTProto client covering a user account, is archived.
- The maintained Rust options for a user account, `rust-tdlib` and `tdlib-rs`, are wrappers over TDLib, a C++
  library the service would have to build and ship, with releases far apart.
- `teloxide`, the maintained Rust Telegram library, covers the Bot API only, which the service already speaks
  itself.
