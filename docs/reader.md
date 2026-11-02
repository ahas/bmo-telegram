# The reader: the owner's account

The reader is the bun process in `reader/` that holds the owner's own Telegram account over MTProto. Nothing
calls it but the service, over its seam (`docs/boundary-api.md`, 「The seam between the service and the
reader」), and it reaches nothing but the service and Telegram.

## What it is for

- **The other bots' words.** The Bot API never shows a bot what another bot says. The reader reads every new
  message another bot writes in a room the receiver listens to and hands it to the service, which hands it to
  the receiver as read in the `human-reader` mode. A message with no room, and one the provider's own bot
  wrote, are not read in.
- **A person's words in a room the bot is not in,** where the service says they are the reader's to hand
  over: the receiving side asked for the room to be read (`docs/telegram.md`, 「The rooms」). A person's words
  anywhere else are the bot account's to read, or nobody's.
- **The owner's own words in such a room,** whatever device they were typed on, are handed over as anyone's
  are, as the owner's: under the owner's number, `BMO_OWNER_ID`, the number the bot account knows the owner
  by. Only the messages the reader's own sends went out as are not, since the sender holds them already: each
  is noted in memory, by its room and the number the account gave it, for five minutes and at most two
  hundred to a room, and a message of the owner's that comes while a send into its room is still going waits
  for that send to end, gone out or failed, before it is matched. In a room the bot is in, the owner's words
  are the bot account's to read, and the reader hands none of them over.
- **What the Bot API will not fetch.** A picture or a file past the Bot API's ceiling for a bot, and a picture
  fetched after the fact for a turn that answers it, are fetched through the reader and answered as bytes.
- **The owner's own words.** A send in the owner's voice goes out in the owner's account, never by the bot:
  the words and the files the bot's send would carry, its markup sent as HTML, the words sent plain where the
  account refuses the markup. A reply the send names is found on the account by the message's number; one it
  has no line under is refused `unreachable`. Each send answers the numbers its messages went out as and the
  words Telegram holds for each, by which the service knows the bot account's sight of them as their echo.
- **The rooms of the account.** Every chat the owner's account is in is listed, for a person to choose a room
  among them; one the bot is not in is read-only (`docs/telegram.md`, 「The rooms」).
- **Contacts and conversations started.** The account's contacts are listed, and a one-to-one or a group with
  them is started in the owner's account, which the bot cannot do (「Contacts and starting a conversation」).
- **History.** The past messages of the rooms the receiver names are walked back and handed over as history.
- **Profile pictures.** The pictures of every room and every sender the provider has seen are kept current.

## What it hands over

- **A message with words only** goes as a live message.
- **A picture,** or an image sent as an uncompressed file, goes as a live message carrying its bytes, as words
  do; where the bytes could not be had, without them.
- **Any other file** — a document, a sound, a video, a sticker — goes at once as a live message saying
  `gathering`, its bytes following as a `gathered` patch, kept with the file's kind or not kept with the
  policy's reason.
- **A wordless message** is handed over under the placeholder of its kind, from `policy/telegram.toml`, the
  same words the bot account's reading uses; the words a message carried go as `spokenText`.
- **Telegram's own words and the custom emoji among their marks** go as the context's `metadata`.
- **Every time goes in epoch milliseconds,** as the protocol states it: Telegram's own `date` counts seconds,
  and the reader multiplies it once.
- **A message's files are listed in the message itself** where they are already gathered, and a message whose
  file is still on its way says `gathering`, its patch following.
- **Every file it hands over or fetches carries its id,** Telegram's own number for it: `photo-` and the
  photo's number, or `document-` and the document's, so the same file crosses under the same id each time.
- **A file with no name of its own** is named for what it is: a sticker by its document, a video, a round note
  or a GIF by which it was, a sound by whether it was a spoken note, each with the extension its type implies.

## Connecting

- **One connection on the owner's account.** A session must not be used from two places at once, so the live
  reading, the history walk, the profile pictures and the owner's sends share one client, and a lock file in
  the provider's data folder, naming the process that holds it, keeps the session to one process. A lock whose
  process is gone is free.
- **It opens the account once the service answers and the lock is its own,** asking again every two seconds
  and saying once that it waits.
- **The connection is kept, and a flood wait is slept through.** A connection Telegram drops, or one that
  does not open, is tried again as often as it takes, and the client's own pings tell a Telegram that has gone.
  A request Telegram answers with a flood wait sleeps the wait it names, however long, and is asked again as
  often as Telegram names one.
- **The session is written by the login script,** `bun run login` in `reader/`, which asks for the phone
  number, the code Telegram sends and the account's password where one is set, and prints the session to put
  in `BMO_TG_SESSION`.
- **Without its three variables the reader stands inactive,** says why on its health, and opens nothing; the
  provider then declares the `human-reader` mode inactive with that reason. A session Telegram no longer
  authorizes is said on its health the same way.

## The rooms of the account

- **Every chat in the account's dialog list** is listed on `GET /reader/dialogs`, the whole list in every
  folder, archived ones included, each under the number the service gives the room and with the title the
  account shows for it: a group's or a channel's name, a person's name for a one-to-one. That is every
  one-to-one, with a person or a bot, every group and supergroup, and every channel the account may post in,
  whether or not the bot is in it and whether or not the receiver has ever seen it.
- **Two dialogs are no room to speak in and are left out:** a basic group that became a supergroup, which is
  listed as the supergroup it became, and a channel the account can only read, being neither its creator nor
  an administrator who may post. The list is asked of Telegram whole and these two are left out by the reader
  itself, by what each dialog's chat says of it, not by the MTProto client's own filters.
- **The owner's one-to-one with the bot** is the chat with the bot as the account sees it, and is listed under
  the owner's number, as the service numbers that room. The account's own saved messages are no room of the
  provider's and are not listed.
- **The list is read again at most every thirty seconds;** a page asked within that time is answered from the
  list last read, so walking the pages does not walk the account's dialogs each time.
- **While the account is not connected** the route answers 503 `not-ready` with the reason the health gives.

## Contacts and starting a conversation

- **The contacts are the account's contact list,** read by MTProto's `contacts.getContacts` and listed on
  `GET /reader/contacts`: each contact's number, the name the account shows for it (first and last name, else
  the handle, else the number), whether it is a bot, and the number of the one-to-one the account has with it
  where its dialog list holds one, the chat with the bot under the owner's number as in the rooms. The
  account itself and a deleted account are not listed. The list is read again at most every thirty seconds, as
  the rooms are.
- **A one-to-one is started** on `POST /reader/start` with `contactId`: a contact of the account comes to its
  room's number and its name, and nothing is sent, since Telegram makes the chat stand at its first message. A
  number that is not a contact is answered `no-contact`.
- **A group is started** on the same route with `title` and `contactIds`, by MTProto's `messages.createChat`
  with those contacts as its members; it comes to the new group's number, as the service numbers a group, and
  the title. Every number must be a contact, else the first that is not is answered `no-contact` and nothing
  is made; what Telegram refuses is answered `refused` with Telegram's own words. The list of rooms is read
  again on the next page asked, so the new group is listed at once.
- **While the account is not connected** both routes answer 503 `not-ready` with the reason the health gives.

## The history walk

- **The rooms walked are the receiver's to name,** each with the oldest message it holds, and the owner's
  one-to-one with the bot among them (`docs/boundary-api.md`, 「Needed from the receiver」). Where the receiver
  does not name them, the walk stands down and says why. A room whose peer the account cannot resolve hands
  nothing over rather than every chat's history.
- **It starts a minute after the account connects,** one room at a time, each to its top before the next; a
  room whose walk reached its top is not walked again.
- **Numbers.** Only a supergroup or a channel numbers its messages for everyone, so its messages keep their
  numbers, and its walk starts below the oldest message the receiver holds. Another room's history is numbered
  below every number the bot account can see, older first, and only what was said before the receiver's first
  message is handed over, so a message read back never takes a number a live message has.
- **Progress is kept per room** in the reader's own store, `reader.db` in the provider's data folder: the
  lowest number reached and whether the top of the room's history was reached. A walk resumes where it left off,
  and pauses between pages of a hundred.
- **What a line out of the history says.** A picture says so, whether or not it was captioned; a message that
  is only words is handed over exactly as it was said. Every line with words or a picture is handed over,
  whoever said it, the owner's own under the owner's number.
- **By hand,** `bun run history --room <chat> [--max <messages>]` in `reader/` walks one room's history while
  the reader's own process is stopped; a room the receiver does not name is refused before the account is
  asked anything.
- **A room's recent past is filled by hand,** `bun run history --room <chat> --days <days>`, while the
  reader's own process is stopped: every line said in those last days, newest first, is handed over as
  history under the number the live reading gives it, so the receiver keeps the lines it lacks and leaves the
  ones it holds, the copies of what it sent into the room among them. Only a room the account numbers as the
  live reading does is filled: a supergroup or a channel, or a room whose people are the reader's to hand
  over; any other is refused before its past is read, and so is a room the receiver does not name. The span
  reaches no further back than the room's live reading: what the walk handed over below the receiver's first
  message stands under the walk's numbers, and a fill reaching it would hand it over again. The walk's
  progress is left as it stands.

## Profile pictures

- **Whose picture is kept:** every room and every person the provider has seen, the bot account among them,
  each once. The owner's one-to-one with the bot wears the bot's picture, and the owner keeps their own where
  they speak.
- **Which picture Telegram shows** is the number of the account's photo, for a person and a room alike, and
  nothing at all where the account shows none. Someone the account knows only as a member of a room is found
  among that room's members.
- **A picture is taken again** only when nobody has taken it yet, when the picture behind it changed, or when
  the file its record points at is gone. A subject that shows none has its file taken down. A file standing
  where it now belongs repairs a record that points elsewhere.
- **One pass over everyone** starts a minute after the reader connects and runs again every six hours. It
  takes what it can, counts the rest, and keeps a room and a person of one number apart. The files are kept in
  `avatars/` in the provider's data folder, the record in the reader's store carries when each was taken in
  epoch milliseconds, and the reader lists them on `GET /reader/avatars`; the service serves them through the
  protocol's avatar routes. Word of the rooms and people whose picture changed goes to the service once a
  pass.

## Why it is TypeScript

The reader stays on the TypeScript MTProto client, `telegram` (gramjs), because no maintained Rust library
covers what it needs: a user account's login, history reading, file download and profile pictures.

- `grammers`, the one pure-Rust MTProto client covering a user account, is archived.
- The maintained Rust options for a user account, `rust-tdlib` and `tdlib-rs`, are wrappers over TDLib, a C++
  library the service would have to build and ship, with releases far apart.
- `teloxide`, the maintained Rust Telegram library, covers the Bot API only, which the service already speaks
  itself.
