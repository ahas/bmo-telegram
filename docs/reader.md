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
- **The owner's own words, and the avatar's.** A send in the owner's voice, or in the avatar's behind its prefix,
  goes out in the owner's account, never by the bot: the words and the files the bot's send would carry, its
  markup sent as HTML, the words sent plain where the account refuses the markup. The typing mark is shown, a
  message the account holds deleted for everyone, the owner's own reaction put on it, a message pinned or unpinned
  for everyone and his own line edited, the same way. A reply the send names is found on the account by the
  message's number; one it has no line that the number names is refused `unreachable`. A line the service names
  by its stamp, the sender, the second and the hash of its words, is looked for among the room's lines of that
  second, for the account's own number for it where exactly one answers. Each send answers the numbers its
  messages went out as under the provider's numbering (「Numbers」) and the words Telegram holds for each, an edit
  the words its line now holds and a pin the number of the service message Telegram wrote for it, by which the
  service knows the bot account's sight of them as their echo.
- **The rooms of the account.** Every chat the owner's account is in is listed, for a person to choose a room
  among them; one the bot is not in is read-only until it is added (`docs/telegram.md`, 「The rooms」).
- **Contacts and conversations started.** The account's contacts are listed, and a one-to-one or a group with
  them is started in the owner's account, which the bot cannot do (「Contacts and starting a conversation」).
- **History.** The past messages of the rooms the receiver names are walked back and handed over as history.
- **Who is writing and who is reachable.** People's typing and their status, which the Bot API never shows a
  bot, are heard on the account and handed to the service (「Typing and presence」).
- **One chat's recent messages.** A dialog is read by its number for the supervisor's road, newest first,
  without a room standing for it and without touching the history walk or the record.
- **A room's members.** Everyone the account lists in a chat, which the Bot API never lists a bot
  (「A room's members」).
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
- **Telegram's own words, the custom emoji among their marks and the sticker a message is** go as the context's
  `metadata`, the sticker in the bot account's reading's shape as far as the account can tell it: its emoji, its
  set's name where Telegram names it, and whether it is animated or a video; `null` where the message is none.
- **What happens to a message afterwards** is handed over too, as the protocol's reaction changes and deletions:
  a message deleted, and a message's reactions as the account sees them, every person it lists with the emoji
  they have on it. A supergroup or a channel numbers its messages for everyone, so such a message is named by its
  number; in any other room the account numbers messages apart from the bot account, so a message is named by its
  stamp, who sent it and when, from what the reader saw of it while it ran. A message the reader never saw while
  running is named by nothing and is passed over.
- **Every time goes in epoch milliseconds,** as the protocol states it: Telegram's own `date` counts seconds,
  and the reader multiplies it once.
- **A message's files are listed in the message itself** where they are already gathered, and a message whose
  file is still on its way says `gathering`, its patch following.
- **Every file it hands over or fetches carries its id,** Telegram's own number for it: `photo-` and the
  photo's number, or `document-` and the document's, so the same file crosses under the same id each time.
- **A file with no name of its own** is named for what it is: a sticker by its document, a video, a round note
  or a GIF by which it was, a sound by whether it was a spoken note, each with the extension its type implies.

## Numbers

- **A supergroup or a channel numbers its messages once for everyone,** so a number of such a room crosses the
  seam as it stands in every direction: a hand-over carries it, a send answers it, a reply or an act names it,
  and a fetch looks the line up by it.
- **Every other room numbers messages per account,** and the bot account's numbers already stand on the
  receiving side for the lines it read and sent there, so a raw number of the owner's account could meet a
  different line under it. Every number the owner's account gave of such a room therefore crosses as the
  provider's own for the line: its account number offset far below every number the Bot API hands out, rising
  with it so older lines still sort first, negative where the account's own is positive. The live reading's
  hand-overs and their gathering patches, the history walk, a room's recent past filled by hand, and the numbers
  the owner's sends answer all go this way; a reaction or a deletion of such a room is named by its stamp, as
  ever, and by nothing the reader never saw.
- **Such a number is turned back wherever it comes home:** before a reply's or an act's number names a line to
  the account, and before a fetch looks one up. The account's own numbers for such a room cross nowhere else.
- **Numbers the receiving side took before this numbering stood** still resolve, as the ledger says each one's:
  one recorded with the bot account's stamp is looked for by that stamp, one recorded as the owner's account's
  stands as it is, and one it cannot tell names no line (docs/telegram.md, 「The owner's voice」).

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
- **Numbers.** A room that numbers its messages for everyone keeps them, and its walk starts below the oldest
  message the receiver holds. Another room's lines cross under the provider's numbering (「Numbers」), older
  first, and only what was said before the receiver's first message is handed over, so a message read back never
  takes a number a live message has.
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

## Typing and presence

What the service tells the receiving side of them is `docs/telegram.md`, 「Typing and presence」; this is how the
reader hears them.

- **Which rooms tell.** The service says of each room whether its people's typing and presence are the reader's
  to tell (`tells` on `POST /reader/listened`): a room the receiver listens to, and, where the bot is not in it,
  only one the receiver asked to be read. The reader keeps each answer for thirty seconds, so a person typing on
  does not ask it again each time.
- **A typing action** is Telegram's update of a user typing in a private chat, a group, or a supergroup or
  channel, each naming its room as the service numbers rooms. An action of writing a message (typing,
  recording or uploading a voice note, a round note, a video, a photo or a file, choosing a sticker, a contact
  or a place) is a mark `on`; a cancel action is the mark off; any other action, such as an emoji's animation
  or speaking in a call, is no mark. One whose sender is not a user, the owner's account's own and the
  provider's bot's are passed over.
- **The people told of** are gathered from the rooms the receiver names (`GET /reader/rooms`) whose people the
  service says tell: the person of a one-to-one, and a group's or a channel's members, as many as the account
  is shown, up to five hundred a room, as the profile pictures read them. The account itself and the
  provider's bot are left out.
- **A whole list** of their statuses, as each member came with it, is handed over fifteen seconds after the
  account connects, once the service counts the account connected, and again every ten minutes, each time with
  the people gathered again. A person Telegram gives no status for, as for a bot, is left out and so stands
  unknown. Where the receiver does not name its rooms, no list is handed over that time.
- **A status update** of a person told of is handed over at once, one person not making a whole list; one of
  anyone else is passed over.
- **Online lapses by itself.** A status online until a moment is handed over online, and, unless another
  status comes first, offline at that moment with that moment as last seen.

## One chat's recent messages

- **A route on the live account:** `GET /reader/messages?chat=<id>&max=<n>&since=<epoch ms>` answers one
  dialog's recent messages, newest first, from the connection the reader already holds; no second MTProto
  connection is opened.
- **Each line** carries the number the account gives the message, as this route reads the account's own view and
  not the receiving side's record (「Numbers」 says what crosses), the sender's number and the name the account
  shows, the moment in epoch milliseconds, and the words. A message that is not words alone carries the
  placeholder its kind stands under in `policy/telegram.toml`: a picture, a file, a sound, a voice note or any
  other non-text.
- **`max`** is how many lines at most, fifty where none is named and two hundred the most; **`since`** is the
  earliest moment in epoch milliseconds, and no line older than it is answered.
- **A chat number the account cannot resolve is refused** `not-found` with the reason, rather than read as a
  search over every chat the account is in.
- **While the account is not connected** the route answers 503 `not-ready` with the reason the health gives.
- **No room is made and nothing is sent.** The route reads and answers; the dialog list, the history walk and
  the record of what was seen are left as they stand.

## A room's members

- **A route on the live account:** `GET /reader/members?chat=<id>` answers everyone the account lists in one
  chat, from the connection the reader already holds, each with their number and the name the account shows for
  them: first and last name, else the handle, else the number.
- **What Telegram shows the account is what is answered:** every member of a group; every member of a supergroup
  or a channel, page after page, where the account may see them, and its administrators alone where it hides
  its members from the account. A one-to-one is its person, and the owner's one-to-one with the bot is the bot.
- **The account itself stands under the owner's number,** as its words do, and is added to a one-to-one, which
  Telegram answers with its person alone. A deleted account is left out.
- **A chat number the account cannot resolve is refused** `not-found` with the reason, and **while the account is
  not connected** the route answers 503 `not-ready` with the reason the health gives.
- **Nothing is kept or sent.** The route reads and answers, each time it is asked.

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
