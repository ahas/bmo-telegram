# The Telegram provider

What this provider is to the receiver: what it declares, how it reads a room, what each send does, and the
dialect its rooms read. Its routes and configuration are `docs/boundary-api.md`; the owner's account is
`docs/reader.md`.

## The declaration

| Member | Value |
|---|---|
| `key` | `telegram` |
| `label` | `Telegram` |
| `configured` | null where the bot token is set, else `BMO_BOT_TOKEN not set` |
| `modes` | `bot-account`, `human-reader` and `human-avatar`, below |
| `can` | the capabilities below |
| `dialect` | `telegram-html`, below |
| `shows` | null |
| `numbering` | `native` for rooms, people and messages: Telegram's own numbers, used as they are |
| `speakers` | true where a supervisor has a bot of its own (「A bot for each supervisor」), false otherwise: a send naming no speaker goes out as the bot account, or in the owner's account in the owner's voice or the avatar's |

### Modes

| Voice | Reach | Standing | `canReplyTo` |
|---|---|---|---|
| `bot-account` | `all-messages` | `active` | true |
| `human-reader` | `all-messages` | `active` while the reader is connected; `inactive` otherwise, with the reader's reason, `BMO_TG_API_ID, BMO_TG_API_HASH, BMO_TG_SESSION not all set` where they are missing | true |
| `human-avatar` | `all-messages` | as `human-reader`: `active` while the reader is connected, `inactive` otherwise with the same reason | true |

- **The bot account reads every message of every room it is in,** and is what the provider runs on with
  nothing else configured.
- **The owner's account reads what the bot account cannot,** the other bots' words, which the Bot API never
  shows a bot, and the bytes behind a picture or a file past the Bot API's ceiling, **and speaks the owner's own
  words**: a send in its voice goes out in the owner's account, never by the bot (「The owner's voice」). Such a
  send can be pinned to a message, since the account finds the line by the message's number.
- **A room can be answered as the owner, in the avatar.** The `human-avatar` mode is the owner's account speaking
  the agent's words, each behind the avatar prefix, so a reader of the room tells them from the owner's own
  (「The avatar」). It stands on the owner's account alone, so it is active exactly while the reader is, and a
  room of the bot's added in the avatar mode is answered through the owner's account rather than by the bot.
- **The bot answers as itself** in every room of the bot's that is not answered in the avatar: a send that
  names no voice, or `bot-account`, goes by the bot.

### Capabilities

All twelve send verbs, and `replyTo`, `fetchMedia`, `avatars`, `history`, `roomList`, `contactList`,
`memberList`, `startChat`, `startGroup`, `peopleTyping` and `presence`. All but the send verbs, `replyTo` and
`memberList` are the owner's account's: each is declared only while the reader mode is active, and `history` and
`avatars` only while their part stands (`GET /status`). `memberList` is declared always, since the bot account
tells a room's members on its own (「The members of a room」). The rooms `roomList` lists are the bot's and the owner's
account's (「The rooms」); the contacts and the conversations started are the owner's account's alone
(「Contacts and starting a conversation」); who is writing and who is reachable are heard by the owner's
account alone (「Typing and presence」).

### The rooms

- **A room is the bot's where the bot account is in it:** the owner's one-to-one, a one-to-one whose person has
  written to the bot, and any group or channel Telegram says the bot is a member of.
- **A one-to-one is the bot's only once its person has written to the bot** in their private chat with it, as the
  bot account read the message; the provider keeps a record of who has, and Telegram is not asked. Telegram lets
  a bot speak to a person only after the person has spoken to it, and it knows a person the bot merely shares a
  group with as well as one who wrote to it, so that knowing is no sign of a chat. A one-to-one of the owner's
  account with anyone else, the bot met in a group or never at all, is the owner's account's.
- **For a group or a channel** Telegram is asked once and its answer stands for ten minutes; a message the bot
  account reads in a room lets go of the answer kept for it, so a room the bot has just joined is asked again.
  Where Telegram cannot be asked, a room the provider has seen stands as the bot's.
- **What Telegram says of a room is kept, so asking of a room again asks Telegram nothing.** A room's title as
  Telegram tells the bot stands for ten minutes, and the list of the owner's account's chats for one minute. Where
  Telegram did not answer, that stands for half a minute too, so a Telegram that is slow or out of reach is asked
  once in that span however many rooms are asked of. One asker at a time asks Telegram for a title, and one for the
  list: the others wait for that answer and take it. The owner's account is his own, and a list read for every
  room every time a console shows its rooms is what gets an account limited.
- **A room of the owner's account the bot is not in is the owner's account's alone.** It is listed and answered
  as a room of the provider's own, so it can be added. Until the receiving side reads it (below) it is
  read-only: its `modes` are `human-reader` alone, and only the owner's own words, a send naming that voice, go
  into it. Once the receiving side reads it, which it does for a room added on this provider whatever its mode,
  it can be answered as the owner as well: its `modes` are `human-reader` and `human-avatar`, and a send naming
  the avatar goes into it through the owner's account (「The avatar」). A send in any other voice, or naming none,
  is refused `platform` with the policy's `read_only`, whatever its verb, so the bot never answers in such a
  room, and so is an avatar send into a room the receiving side does not read.
- **A room the bot is not in is read through the owner's account only where the receiving side asks for it.**
  Other bots' words are read wherever the room is listened to, as in any room. A person's words there are
  read and handed over, as `live` in `human-reader`, only where the receiving side asked for the room to be
  read, so the owner's other chats never flow. Both sides of such a room are handed over: the owner's own
  words there, typed wherever the owner typed them, go as the owner's, under `BMO_OWNER_ID`, but for the
  messages a send in the owner's account went out as, which the sender holds already (`docs/reader.md`, 「What
  it is for」). The request is the `read` of the room as the receiving side answers it; whether the room is
  listened to is no such request, and a receiving side that does not answer, or holds nothing of the room,
  asks for nothing.
- **A room's `modes`** are the voices the declaration has active in a room of the bot's, `human-avatar` among
  them while the reader is connected; in a room the bot is not in, `human-reader` and `human-avatar` where the
  receiving side reads it, and `human-reader` alone where it does not.
- **A room's `identity`** is the bot's number in a room of the bot's, and in a room the bot is not in that the
  receiving side reads, where it is the number the agent's words in the avatar stand under (「The avatar」); it
  is null in a room the bot is not in that the receiving side does not read, since no words of the provider's
  own stand there but the owner's. A hand-over's room facts carry the same identity as the room's answer.
- **A room the bot is not in that the receiving side reads carries no `privateTo`,** one-to-one or not: it may be
  answered in the avatar voice, where the person in it speaks with the owner and not with the agent.
- **The list** holds the bot's rooms, the owner's one-to-one and every room seen that the bot is in, and every
  chat of the owner's account (`docs/reader.md`, 「The rooms of the account」), each room once: a chat the bot is
  also in is the bot's, under the account's title for it, and a room of the bot's the account does not list is
  titled as Telegram tells the bot. Each is shaped as `GET /rooms/{roomId}` answers it: a one-to-one, with a
  person or a bot, carries that one's number as `privateTo`, and every other chat, group, supergroup or channel,
  is `group` with no `privateTo`. Whether the bot is in each room is asked of Telegram eight rooms at a time,
  each answer kept as above, so walking the pages of a long list asks Telegram about each room once.

### The members of a room

A room's members are everyone either account can tell is in it, each once, named by their Telegram number, the
sender number their messages carry, since the provider numbers people as Telegram does.

- **The owner's account lists everyone in a room it is in:** every member of a group, and every member of a
  supergroup or channel as Telegram shows them to the account, which for one that hides its members from all but
  its administrators is its administrators alone. A one-to-one is its person and the owner. The account itself
  stands under the owner's number, `BMO_OWNER_ID`. It is asked where the reader is connected and the room is among
  the account's chats (`docs/reader.md`, 「A room's members」).
- **The bot account tells who it can in a room it is in.** The Bot API lists no group's members, so the bot gives
  the room's administrators and every sender the provider has seen in the room, each asked of Telegram one by one
  and kept only where Telegram says they are still a member: a person who left, was removed or is no longer known
  to the room is left out. The provider keeps each room's senders in the record of what it has seen: the sender
  of every live message and every line of history it hands over, and, each time the members are asked where the
  owner's account is in the room, the senders of the room's last two hundred lines as the account reads them. So a
  member Telegram leaves out of the account's list is still found once they have spoken. A one-to-one of the bot's
  is its person and the bot.
- **The account's list comes first, in Telegram's order, then whoever the bot told that it did not.** A person
  is named by their first and last name, else their handle, else their number; no status line is given.
- **A room neither account is in, or whose members neither could read,** is answered `not-found` with the reason
  of each account that tried, so the receiving side says the provider holds none of them.

### Contacts and starting a conversation

- **The contacts are the owner's account's,** its Telegram contact list as the reader reads it
  (`docs/reader.md`, 「Contacts and starting a conversation」): each under the person's own number, with the
  name the account shows, `bot` for a bot account and `person` for any other, and as `roomId` the one-to-one the
  account already has with the contact, numbered as the service numbers rooms, or null. The bot account has no
  contacts.
- **The bot account cannot start a one-to-one.** Telegram lets a bot speak to a person only once the person has
  spoken to it, so every conversation is started by the owner's account, through the reader.
- **A one-to-one started is an owner-account room,** as any room the bot is not in (「The rooms」): read-only,
  `human-reader` alone and no `identity` until the receiving side reads it, unless the bot is already in a
  one-to-one numbered the same, which is then the bot's. Starting it sends nothing: Telegram makes a one-to-one stand only once a message is in it, so
  the room answered is the one the first message will make, under the person's number, titled with the
  contact's name. A one-to-one the account already has comes to that room.
- **A group started is a Telegram group the owner's account makes,** under the title asked, with the contacts
  asked as its members; its people see it made. It is a room of the bot's where a contact brought in is the bot,
  and read-only otherwise.
- **A start naming a number that is not a contact** of the account is answered `not-found`, and one Telegram
  refuses is refused `platform` with Telegram's own description. While the reader mode is not active neither
  capability is declared, so bmo-chat refuses the start before the provider is asked.
- **A room of a contact is known before its first message:** `GET /rooms/{roomId}` answers the one-to-one with a
  contact of the account read-only, where neither the bot nor the account's dialog list has it yet.


## The app registration

- **The owner's account reads through an app registered with Telegram.** The reader speaks MTProto, which takes an
  application's id and hash, `BMO_TG_API_ID` and `BMO_TG_API_HASH` (`docs/boundary-api.md`, 「Configuration」), made at
  https://my.telegram.org. The provider declares that registration, its address and the two values it gives, as
  its declaration's `registration` (bmo-chat's `docs/contract.md`), so a person setting their Telegram account is told
  of it first (`policy/telegram.toml`, `registration_url` and `registration_fields`).

## Reading a room

- **The bot account long-polls the Bot API.** It first asks who it is, asking again until Telegram answers,
  so a message of its own is known by its number; until then the number is read off its token. Updates are
  read one at a time, in order; one that fails is said and the next is read.
- **A call to the Bot API waits for its answer.** The long poll is held for Telegram's own thirty seconds and
  asked again as soon as it returns; a call ends on its answer or on a connection that breaks, and TCP
  keep-alive on every connection tells a Telegram that has gone from one still answering. A picture fetched
  from a web address waits the same way.
- **A reaction on a message is handed over as the message's reactions changed** (bmo-chat's `docs/contract.md`,
  「The receiving side, as a provider sees it」): the bot account asks for reaction updates beside messages, and
  each names the message by the bot account's own number and the person by theirs, with every emoji they have on
  it now, a custom emoji left out. Telegram tells a bot of reactions only in a room where it is an administrator.
- **The rooms the bot account listens to** are the owner's one-to-one and every room whose number the chat
  protocol carries whole, within the integers JavaScript counts exactly. A message from any other room has its
  words handed over and nothing else: no file, no picture and no expression is gathered.
- **What a message carries beyond words** is handed over as a placeholder in the text, the platform's English
  name of the kind with its particulars (a photo, a location, a poll, a sticker with its emoji and set, a
  forwarded message's origin, a member joining or leaving, a pin, a call), with the words typed under it as
  the context's `spokenText`. The placeholders are this repository's policy (`policy/telegram.toml`).
- **People joining and a person leaving are handed over as lines** (bmo-chat's `docs/contract.md`, 「Lines」),
  the placeholder staying the message's text for a reader that does not render a line. Telegram's members joining
  cross as `people-joined`, each member with their number and the name they go by; a member leaving crosses as
  `person-left`, that member alone. The one Telegram names as the message's sender is the line's `by` only where
  they are not among the people who came or the person who went, so a person who joins by a link or leaves of
  their own accord carries no `by`, and one added or removed by someone else does. A joining or a leaving
  Telegram says was done on behalf of a chat, by an anonymous administrator, carries no `by` either: its
  sender is a stand-in Telegram writes for the chat, and nobody the room knows.
- **Telegram's own payload** for a message of a listened room crosses as `metadata`: its words, their
  formatting, its sticker and the file it carries, each as Telegram wrote it and in that order.
- **Every time crosses in epoch milliseconds, as the protocol states times.** Telegram's own `date` counts
  seconds, so the reading multiplies it once, and nothing further down divides it. Every file a message carried
  crosses with its kind as far as Telegram says it: a picture, a video, a round video, an animation, an audio
  file, a voice note, a sticker or a document.
- **A picture goes as a `message` carrying its bytes,** fetched before the message is handed over, as words do;
  where the bytes could not be had, without them. Telegram's largest size is taken, and a picture sent as a
  file is still a picture; an SVG is a file. A picture past the keeping threshold, or one with transparency,
  is brought inside the keeping dimension, laid on white and written as JPEG.
- **Any other file goes as an `attachment` saying `gathering`,** at once, its bytes following as `gathered`
  under the file's own name, or `not-kept` with the policy's reason. A file with no name of its own is named
  for what it is: a spoken note, a sound, a video, a round note or a GIF.
- **A sticker and the custom emoji among a message's words** are gathered as the message's files: the message
  says `gathering`, and the assets follow as `gathered`. Each asset is the file Telegram keeps for it, so a room
  can draw it as the platform does: a still one as WebP, a video one as WebM and an animated one as its TGS
  scene. The placeholder stays the message's words for whatever cannot draw the sticker.
- **A file past the Bot API's ceiling for a bot's download,** 20,000,000 bytes, picture or other, is fetched
  through the reader; with no reader standing, a picture goes without its bytes and another file is not kept.
  This ceiling is the provider's own concern in reading a room, and no declaration carries it: the dialect's
  file limit is the larger one a bot may send.
- **A file gathered past that ceiling is written into the provider's spool folder** and handed on as a file kept
  there (`docs/boundary-api.md`, 「Configuration」), so it crosses as a part of the body it is gathered into and
  the receiving side writes it to its own disk as it arrives rather than holding it whole in its memory. The
  provider removes the file once the receiving side took that body. A picture is redrawn for keeping and goes as
  its bytes wherever it was fetched from, since keeping reads the whole picture, and the refetch of a message's
  files answers them inline as the protocol states.
- **Every file a message carried crosses with its id,** the same each time the same file crosses. The bot
  account names a file by Telegram's `file_unique_id` for it (a picture's by its largest size's), and where a
  file comes with none, by its place in the message: `picture`, `file`, `sticker`, or `emoji-` and the custom
  emoji's number. The owner's account names a file by Telegram's own number for it, `photo-` or `document-`
  and the photo's or the document's number. A picture or a file the bot account handed over is fetched again
  under the id it crossed with, which the provider keeps in its data folder; one it never handed over is
  fetched under the owner's account's id.
- **A send of several messages,** an album, is named by the first of its messages the provider read, in its
  own room, and every message of it carries that number as `album`.
- **An edit** is handed over as `edited` with the words, their formatting and the files it carries that were
  not gathered lately as the same files; a file that changed under its name is gathered again.
- **A group the bot leaves** is handed over as the service message Telegram sends for it, a `person-left` line
  naming the bot and, where someone took it out, them as `by`; a group moving to a
  supergroup's number is no message, and the reading goes on.
- **The bot's own messages** are not read back in: a file it sent is the sender's to keep, and a pin it made
  is nobody's word.
- **A message a send in the owner's account went out as,** in the owner's voice or the avatar's, is not read
  back in either, nor an edit or a pin such a send made: the sender holds it already, so the bot account's sight
  of it is that send's echo (「The owner's voice」).

## Typing and presence

Who is writing in a room now and who is reachable now, told to the receiving side as bmo-chat's
`docs/contract.md` states them (「Typing and presence」). Neither is a message.

- **The owner's account hears both; the bot account hears neither.** The Bot API hands a bot no person's typing
  and no person's status, so `peopleTyping` and `presence` are declared exactly while the reader mode is active,
  and the reader is what hears them (`docs/reader.md`, 「Typing and presence」). Telegram tells them only to a
  session that stands online, so the reader keeps the owner's account online while it holds it, unless the
  deployment switches that off.
- **Only rooms the receiving side listens to are told of.** In a room the bot is not in, which stands read-only
  until the receiving side reads it (「The rooms」), they are told only where the receiving side asked for the
  room to be read (`read`).
- **Never told of:** the owner's account, which the provider reads by, and the provider's own bot, whose typing
  is the answerer's.
- **A typing mark** is a person's typing action in a private chat, a group or a channel, `on`, its life the six
  seconds of the policy's `[typing] lasts_ms`. Telegram tells the action again every few seconds while the
  person goes on, and each is told as it comes; a cancel action is told off.
- **A person's presence** is told for the people who stand in such a room: the person of a one-to-one, and the
  members of a group or a channel as far as the account may list them. Telegram's status maps as below, each under
  the person's own privacy setting:

| Telegram's status | `state` | `lastSeenAt` |
|---|---|---|
| online, until a moment it lapses | `online` until that moment, then `offline` | null while online; the moment it lapsed once offline |
| offline, with the moment last seen | `offline` | that moment |
| recently, within a week, within a month, or a long time ago | `offline` | null |

- **A whole list** of everyone told of is told once the account has connected and again every ten minutes, so a
  receiving side that restarted, or a room added since, stands on the present; between them each change is told
  as Telegram tells it.

## A bot for each supervisor

- **A supervisor may speak through a bot of its own.** The file `BMO_TELEGRAM_SUPERVISOR_BOTS` names holds one
  line a supervisor, its name, `=`, and its bot's token or the name of a file holding that token alone, read as
  each line's key before its first `=` and its value after it. A value that is no token (digits, `:`, then the
  token's letters) names a file, beside the bots file where it names no folder, and its content, blank space at
  its ends left out, is the token; so a token saved as a secret of its own is named rather than copied. Where it names any, the declaration's `speakers` is true, so the receiver names the
  supervisor a send speaks as.
- **A send naming a supervisor goes out through that supervisor's bot** in the bot's voice, and its message is kept
  in the ledger as the service's own bot's are, so its echo is known. Where that bot cannot send in the room — it is
  not in it, or Telegram turns it down — the words go out through the service's own bot instead. An act in the
  supervisor's own name, a typing mark, a reaction, a pin, an edit or a delete, is that bot's alone: its refusal is
  the answer, and the service's bot never does it in its place, since a typing mark or a reaction there would show
  the service's bot, and Telegram lets only the bot that sent a message edit or delete it. A supervisor with no bot
  of its own speaks through the service's own bot.
- **A speaker that names no supervisor is refused** `speaker-not-allowed`, as another account's words are, so the
  receiver sends them in the service's own voice with the speaker's name before them. A send in the owner's account
  names no speaker.
- **A supervisor's bot reads its one-to-ones alone.** The rooms' words, every bot's among them, are the service's
  own bot's and the reader's to read, so a supervisor's bot reads nothing of a group. Each supervisor's bot is
  long-polled for the messages and edits of its private chats, and each such one-to-one is a room of its own:
  Telegram numbers a person's private chat with every bot by that person, so a person's one-to-one with the
  service's bot and with a supervisor's bot would share one number. The provider gives each one-to-one with a
  supervisor's bot its own room, numbered down from -8100000000000, below every number Telegram gives a chat,
  and keeps which supervisor and person each stands for in `supervisor-dms.jsonl` in its data folder, so a
  restart gives it the room it had. Such a room is told to the receiver as a one-to-one with that person whose
  bot is that supervisor's, its members the person and the bot, so the supervisor answers it as its own; every
  send to it goes out through that bot to the person's private chat, whatever voice or speaker it names, and a
  file in it is fetched with that bot's token.

## Sends

Every send is one verb, answered with the message it went out as or acted on. A send Telegram turns down for
what it asked — any answer of its in the 400s but its flood limit — is refused with the platform's own English
reason; a send about a message Telegram no longer has, or will never let change again, is refused and marked
`gone`. A send Telegram could not answer at all — no answer, a connection that broke, its flood limit where
the verb does not wait it out, or its own fault — is the send's failure, in the protocol's error shape, and is
never offered again.

| Verb | What goes out |
|---|---|
| `text` | the markdown in the dialect's markup, in as many messages as it takes, the first answering `replyTo`; folded where the send asks for a fold and the text is long enough for one (「The fold」); without a notification where `silent`; plain text where Telegram refuses the markup. A part that does not go ends the send there |
| `photo` | a picture by web address, or by a file path under the picture root, tried by address first and uploaded where Telegram would not fetch it, redrawn for upload where Telegram would not take it as it is; the caption in the dialect's markup, and plain where Telegram refuses the markup. A picture that does not go is refused as `no-picture-sent`, its caption and web address sent as text in its place |
| `album` | pictures by address or handed over as files, up to ten to an album, a longer list split evenly so that no album holds one; the caption and `replyTo` with the first album; one picture left goes as a `photo`. Each picture that did not go is a `missed` picture with why, and an album none of which went is said as its caption and web addresses; where none of them went at all, the send is refused as `no-picture-sent` |
| `document` | a file with its name |
| `audio` | a sound file with its name, under the type its first bytes read as |
| `voice` | a voice note, the first part of the reading its caption as an expandable quote and the rest quiet replies under it, or its own caption where there is no reading; where Telegram refuses the caption, the voice goes again bare and the reading follows as messages under it. A reading that did not all go is the send's failure, naming the voice that went |
| `sticker` | for an emoji, a sticker of the configured set that carries it, picked at random among them, refused where the set carries none; for an item, the sticker whose Telegram file id is the item's `id`, which asks for no set |
| `reaction` | the emoji on the message; an empty emoji takes the reaction off |
| `pin` | the message pinned, quietly where `silent`, or unpinned |
| `typing` | the bot's chat action, a mark of about five seconds the Bot API cannot take off, so taking it off asks Telegram nothing; for the answerer alone (below) |
| `edit` | the message's words; an edit to the words the message already says is no failure |
| `delete` | the message |

- **A file goes up in the Bot API's own multipart form,** the part carrying its bytes. Bytes handed over in
  memory go as they are; a file that arrived as a part of a multipart send and stands on the provider's disk is
  streamed off that disk as it goes up, so a large file is never held whole. The size Telegram is told for such a
  part is the file's own. A sound's type is read off its first bytes wherever those stand. A send naming a file
  without its bytes is refused with the reason the bytes were named for.
- **The configured sticker set is the deployment's emotion set.** It holds one sticker for each emotion,
  entered with the emoji that stands for that emotion, so a sticker send's emoji asks for that emotion's
  sticker; a set may hold more than one for a feeling, and one of them is picked. A changed emotion list
  wants a new set, named in `BMO_TELEGRAM_STICKER_SET` (`docs/boundary-api.md`, 「Configuration」).
- **A file or a picture answering `replyTo`** still goes where Telegram no longer has the message it answers;
  so does a text, unpinned.
- **Flood limits** are waited out as Telegram asks: an album and each part of a voice's reading are asked
  again after the wait Telegram names, however long, as often as it names one, until they go.
- **A picture named by anything but a web address or a file under the picture root** is not sent: it is
  missed as `not-an-address`, `outside-data-root` or `missing-file`. With no picture root configured, no file
  path is sent. The file and the root are both resolved to where they really stand before either is read, so a
  symbolic link inside the root that reaches a file outside it is `outside-data-root`.
- **A person's typing goes only in their own voice.** A `typing` send in the bot's voice is the answerer's,
  shown as the bot's; one in the owner's voice is his own, shown in his account (「The owner's voice」). A
  `typing` send in the bot's voice that names an account (`accountId`) is that account's own typing, and the
  provider holds no Telegram account of anyone but the owner, so it is refused `platform` with the policy's
  `typing_account` rather than shown as the bot writing.
- **What went is not kept here.** Keeping a copy of a send is the sender's, under the number the result gives.
  The provider notes only, for a few minutes, the messages, edits and pins a send in the owner's account made, to
  know their echo (「The owner's voice」).

### Refusals

Every refusal is one of bmo-chat's refusal codes under the label `Telegram`. The provider's own reasons are
English lines of `policy/telegram.toml`, each the detail of a `platform` refusal:

| When | Code | Detail |
|---|---|---|
| an act on a message Telegram no longer has, or will never let change again | `platform`, marked `gone` | `message_gone`, with Telegram's own description |
| an act in the owner's account on a number that names no line there (「The owner's voice」) | `platform`, marked `gone` | `message_gone`, with the policy's `line_unknown` or the reader's words |
| a sticker the configured set does not carry | `platform` | `sticker_missing` |
| a sticker where no set is configured | `platform` | `sticker_unset` |
| a send asked to be spoken in a mode that is not running, the owner's voice or the avatar's while the reader does not have the account connected among them, or one the provider does not read by | `platform` | `mode_down`, with the mode's reason or `mode_missing` |
| a send in the owner's account, in the owner's voice or the avatar's, of a verb other than `text`, `photo`, `album`, `document`, `audio`, `typing`, `delete`, `reaction`, `pin` and `edit` | `platform` | `owner_verb` |
| a `reaction` or a `pin` in the avatar's voice | `platform` | `avatar_unmarked` |
| an `edit` in the avatar's voice | `platform` | `avatar_edit` |
| an `edit` in the owner's voice of a line that is not his own | `platform` | `not_own_line` |
| a send in any voice but the owner's own into a room the bot is not in, the avatar's but where the receiving side reads the room (「The rooms」) | `platform` | `read_only` |
| a `typing` send in the bot's voice, or naming none, that names an account | `platform` | `typing_account` |
| any other refusal Telegram gives | `platform` | Telegram's own description |
| every picture of a send missed | `no-picture-sent` | each picture with its reason, below |

A picture that did not go is missed with one of bmo-chat's reasons: `not-an-address`, `outside-data-root`,
`missing-file` and `unfetched` carry the base's own sentences; a picture Telegram refused, one whose last try
failed and one left out of an album that went are `platform`, with the policy's `refused`, `failed` or
`left_out`. A provider that is not configured is answered `not-ready` by the base before it is asked.

### The owner's voice

A send is the owner's own words when it names the voice `human-reader`, as the chat protocol names a send in the
owner's own words, and the agent's words in the owner's account when it names `human-avatar` (「The avatar」); a
send carries no sender of its own. Either goes out in the owner's own account, through the reader, so the room
reads it as the owner's, never as the bot's. A send that names no voice, or `bot-account`, goes as the bot's.

- **The same message the bot's send makes.** The owner's account sends what the bot account would have sent
  for the same send, in the owner's name, the avatar's words behind the prefix:
  - **`text`**: the markdown in the parts the dialect divides it into, each in the dialect's markup, folded
    where the send asks for a fold and the text is long enough for one, and as its plain text where Telegram
    refuses the markup; without a
    notification where `silent`. The first part answers `replyTo`. A part that does not go ends the send there.
  - **`photo`**, **`document`** and **`audio`**: the one file, a document kept whole as a file whatever its
    name, a sound under the type its first bytes read as, the photo's caption in the dialect's markup.
  - **`album`**: files go as the albums Telegram takes: pictures and videos together, documents together,
    sounds together, a GIF alone, each album up to Telegram's ceiling, in the order their first file came. An
    item named by an address is read as a picture of the bot's is. The caption goes with the first album in
    the dialect's markup, its line breaks each a single line feed and its ends trimmed. A file that holds
    nothing, or one past the `[owner]` ceiling, is missed with the policy's `empty_file` or `too_large`; an
    album the account refuses is missed by every file in it, and stops none after it. The first album answers
    `replyTo`.
  - **`typing`**: the typing mark shown in the room as the owner's account's, or taken off.
  - **`delete`**: the message the owner's account holds under the number, deleted for everyone in the room. A
    number the account holds no line under is refused marked `gone`, with the policy's `message_gone`.
  - **`reaction`**, in the owner's own voice alone: the emoji put on the message the owner's account holds under
    the number as the owner's own reaction, an empty emoji taking his off, so the room reads it as his and never
    as the bot's. A reaction the message already carries from him is no failure. A number the account holds no
    line under is refused marked `gone`, with the policy's `message_gone`. In the avatar's voice it is refused with
    the policy's `avatar_unmarked` (「The avatar」).
  - **`pin`**, in the owner's own voice alone: the message the owner's account holds under the number pinned for
    everyone in the room, quietly where `silent`, or unpinned, so the room reads the pin as his and never as the
    bot's. A number the account holds no line under is refused marked `gone`, with the policy's `message_gone`. In
    the avatar's voice it is refused with the policy's `avatar_unmarked` (「The avatar」).
  - **`edit`**, in the owner's own voice alone: the owner's own line the account holds under the number given the
    send's words, as one message in the dialect's markup, never divided and never folded, and as its plain text
    where Telegram refuses the markup. An edit to the words the line already says is no failure. A number the
    account holds no line under is refused marked `gone`, with the policy's `message_gone`; a line that is not
    the owner's own is refused with the policy's `not_own_line`. In the avatar's voice it is refused with the
    policy's `avatar_edit` (「The avatar」).
  - A `pin` or an `edit` Telegram turns down, for want of a right in the room or a message too long among
    others, is refused with Telegram's own words.
  - Any other verb is refused with the policy's `owner_verb`.
- **A reply** is named by the send's `replyToMessageId`, the message's number as the protocol carries it. The
  reader finds that number's line on the owner's account and answers it; where the account has no line that the
  number names, the send is refused `unreachable` rather than sent without the reference the caller asked for.
- **One numbering of the provider's own.** Each account numbers the messages of a private chat or a basic group
  its own way, and the bot account's numbers stand on the receiving side for every line it read or sent there; a
  number of the owner's account crossing as it stands could meet one of those lines' under another line. So in
  every room but one that numbers its messages for everyone, the receiving side holds the owner account's lines
  under the provider's own number for them: the account's number offset far below every number the Bot API
  hands out, rising with it so older lines still sort first, negative where the account's own is positive. What
  a send in the owner's voice answers, what the reader's hand-overs and their gathering patches carry and what
  the history walk gave are all this numbering (`docs/reader.md`, 「Numbers」); the reader turns such a number
  back into the account's own wherever a reply, an act or a fetch names it, and no ledger record is needed to
  understand it.
- **Which line a number names.** The number a reply, a `delete`, a `reaction`, a `pin` or an `edit` names is the
  one the receiving side holds the line under:
  - In a supergroup or a channel the number is everyone's and stands as it is, and so does a number below zero
    of any room, which is the provider's numbering and turns back by its own arithmetic.
  - A positive number of a room the bot account reads is its own for the line the receiving side names, and the
    line was kept in a ledger in the provider's data folder (`docs/boundary-api.md`, 「Configuration」) with its
    stamp (its sender, the second it was sent in and the SHA-256 of its words, the words as an edit last left
    them) so a restart forgets none; such a number is looked for on the owner's account by that stamp, among the
    room's lines of that second, and names the one line from that sender with those words.
  - A number the ledger holds as both accounts' (as lines taken before the numbering above stood can stand), a
    number it does not hold in a room it holds a line of the bot account's for, and a stamp
    the owner's account holds no line or more than one line for, name no line: an act on it is refused marked
    `gone`, with the policy's `message_gone` and the policy's `line_unknown` or the reader's words, and a reply
    to it is refused `unreachable`. In a room the ledger holds no line of the bot account's for, as one the bot
    account does not read, every number names a line of the owner's account and resolves as it reads: by itself
    where it is positive, by turning back where it is not.
- **While the reader does not have the account connected**, or no reader stands behind the service, the send
  is refused `platform` with the policy's `mode_down`, naming the voice the send asked for, and the reader's
  reason. It is never sent by the bot instead.
- **The number a send answers** is the provider's number for the first message it went out as: the number
  everyone knows the line by where the room numbers for everyone, the owner account's number under the provider's
  numbering elsewhere, so the copy the sender keeps stands under a number that cannot meet a line the bot account
  read. An act answers the number it was asked about.
- **Its echo is not handed over.** The bot account sees in its rooms what the owner's account does there, all of
  it from the owner's number, `BMO_OWNER_ID`: a message it sends as a new message, an edit as an edited message,
  and a pin as the service message Telegram writes for it. Each of these a send in the owner's account made is
  noted, in memory and for five minutes, by what it is (a message, an edit or a pin), its room, the number the
  send answered it under and the words Telegram holds for it, a pin's service message holding none. What the bot
  account reads from the owner's number that answers a note of its own kind is that send's echo: the note is let
  go and nothing is handed over, since the sender holds it already. In a supergroup or a channel the number
  answers the note; in any other room the words do, the oldest note first, since there the bot account knows the
  line by a number of its own, which the receiving side does not hold for it. An unpin, a pin Telegram wrote no
  service message for (the message was pinned already) and an edit to the words the line already said show the bot
  account nothing, and are not noted. Anything from the owner that comes while a send in the owner's account
  into its room is still going waits for that send to end, gone out or failed, before it is matched.

### The avatar

A send in the `human-avatar` voice is the agent speaking in the owner's account, so every word of it is marked as
the agent's by the avatar prefix, and the owner's own words, in `human-reader`, never are.

- **The prefix** is the send's own `prefix` where it carries one that is not blank, the speaking agent's name
  tag the receiver hands every send, else the policy's `[avatar] prefix`. A send therefore always has one, and
  the mode needs nothing beyond the reader to stand.
- **Every message the send goes out as carries it.** Each part of a divided text begins with it, the text divided
  short enough that the part and the prefix fit one message; each album's caption begins with it, the prefix
  alone where the send has no caption, so a file sent bare is still marked. Words that already begin with the
  prefix, read without the space after it, go out as they are, never doubled.
- **The prefix reads as written**: it is put before the plain words as it stands and before the markup escaped,
  so nothing in it is read as a mark.
- **`typing` and `delete` carry no words** and so no prefix.
- **A reaction or a pin is never the avatar's.** Neither carries words, so nothing on it could mark it as the
  agent's, and it would read as the owner's own: in the avatar's voice either is refused with the policy's
  `avatar_unmarked`.
- **An edit is never the avatar's.** The account cannot tell a line the avatar sent from one the owner typed, so an
  edit in the avatar's voice could put the agent's words into a line read as his: it is refused with the policy's
  `avatar_edit`.
- **A room the bot is not in takes an avatar send only where the receiving side reads it**, so only a room added
  on this provider; elsewhere it is refused with the policy's `read_only` (「The rooms」). Whether the receiving
  side reads a room is asked of it at the send, as the room's answer asks it.
- **A line the avatar sent is the agent's own wherever it is read back.** A message from the owner's number, in a
  room with an `identity` (a room of the bot's, or one the receiving side reads), whose words begin with the
  prefix is handed over as the agent's: under the room's
  `identity`, in the `human-avatar` mode, with the prefix and the one space after it taken off
  its words and off the words typed under a file. It holds on every road the owner's account's lines come back
  by: the bot account's reading of a new message or an edit, once the echo note has gone or after a restart, and
  the reader's live reading and history walk. A line an echo note still answers is not handed over at all.
- **The prefixes read back are those the sends named,** each learnt as a send carrying it goes and kept in the
  provider's data folder, `avatar-prefixes.txt`, one a line, so a tag is recognised after a restart too, beside
  the policy's own. A line beginning with several is read against the longest, and the whole of it, the agent's
  name with its emoji, is taken off, so no tag stands in the words a reader of the record sees.
- **A prefixed line in a room with no `identity` is the owner's own.** No avatar send goes there, so such a line
  is one the owner typed, and it is handed over as his.
- **An unprefixed line from the owner's number is the owner's own words** wherever it is read.

## The dialect: `telegram-html`

The agent's markdown becomes Telegram's HTML. The dialect has:

| Member | Value |
|---|---|
| `limits` | text 4096, caption 1024 (both counted in UTF-16 units), album 10 pictures, file 50,000,000 bytes, the largest file the Bot API takes from a bot to send |
| `writing` | "This room shows formatting. Write markdown as the answer-shape rulebook describes: the send path turns it into this platform's own markup and escapes what has to be escaped, so never write a tag yourself. An answer folds into an expandable quote only where its send asks for a fold." |
| `foldable` | true: an answer whose send asks for a fold folds into an expandable quote where it is long enough for one, its lead and a short closing line left open |
| `convert` | markdown in, the HTML markup, the same words plain, and whether the markup says more than the plain text |
| `parts` | the markdown divided at block boundaries into pieces each of which fits one message |
| `fault` | an English reason where the markup is not sound HTML Telegram reads: a character left unescaped, a tag Telegram does not take, a tag left open, one closed that was never opened, two crossed, one nested where it may not be |
| `fold` | the folded markup, or null where the text is too short for a fold |
| `visible` | what a reader sees of the markup |

Whether a reply may fold is the sender's decision, carried to the provider as the send's `fold`, as the fixed
protocol states it. The provider applies the dialect's fold where the send asks for one and asks no output gate
of its own.

## The fold

A fold hides the body of a long message behind an expandable quote, its lead and a short closing line left
open, so a room reads the gist and opens the rest. It is for long text only: a text past 600 UTF-16 units or
more than eight lines folds where its send asks for a fold; a shorter text goes open however it was asked, since
a quote that hides three sentences, or the whole message, helps nobody. A text that asked for no fold never
folds. The dialect's own fold (the `fold` reading) answers null for a text too short, and the code's own fold,
where nothing upstream decided, follows the same measure.
