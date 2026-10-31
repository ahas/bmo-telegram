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
| `modes` | `bot-account` and `human-reader`, below |
| `can` | the capabilities below |
| `dialect` | `telegram-html`, below |
| `shows` | null |
| `numbering` | `native` for rooms, people and messages: Telegram's own numbers, used as they are |
| `speakers` | false: a send names no speaker; it goes out as the bot account, or as the owner's in the owner's voice |

### Modes

| Voice | Reach | Standing | `canReplyTo` |
|---|---|---|---|
| `bot-account` | `all-messages` | `active` | true |
| `human-reader` | `all-messages` | `active` while the reader is connected; `inactive` otherwise, with the reader's reason, `BMO_TG_API_ID, BMO_TG_API_HASH, BMO_TG_SESSION not all set` where they are missing | true |

- **The bot account reads every message of every room it is in,** and is what the provider runs on with
  nothing else configured.
- **The owner's account reads what the bot account cannot,** the other bots' words, which the Bot API never
  shows a bot, and the bytes behind a picture or a file past the Bot API's ceiling, **and speaks the owner's own
  words**: a send in its voice goes out in the owner's account, never by the bot (「The owner's voice」). Such a
  send can be pinned to a message, since the account finds the line by the message's number.
- **No room is answered as the owner.** The provider declares no `human-avatar` mode, so a room is never added to
  be answered in the owner's account; the owner's account carries the owner's own words alone.

### Capabilities

All twelve send verbs, and `replyTo`, `fetchMedia`, `avatars`, `history`, `roomList`, `contactList`,
`startChat` and `startGroup`. All but `replyTo` are the owner's account's: each is declared only while the reader
mode is active, and `history` and `avatars` only while their part stands (`GET /status`). The rooms `roomList`
lists are the bot's and the owner's account's (「The rooms」); the contacts and the conversations started are the
owner's account's alone (「Contacts and starting a conversation」).

### The rooms

- **A room is the bot's where the bot account is in it:** the owner's one-to-one, a one-to-one whose person has
  written to the bot, and any group or channel Telegram says the bot is a member of.
- **A one-to-one is the bot's only once its person has written to the bot** in their private chat with it, as the
  bot account read the message; the provider keeps a record of who has, and Telegram is not asked. Telegram lets
  a bot speak to a person only after the person has spoken to it, and it knows a person the bot merely shares a
  group with as well as one who wrote to it, so that knowing is no sign of a chat. A one-to-one of the owner's
  account with anyone else, the bot met in a group or never at all, is the owner's account's and read-only.
- **For a group or a channel** Telegram is asked once and its answer stands for ten minutes; a message the bot
  account reads in a room lets go of the answer kept for it, so a room the bot has just joined is asked again.
  Where Telegram cannot be asked, a room the provider has seen stands as the bot's.
- **A room of the owner's account the bot is not in is read-only.** It is listed and answered as a room of the
  provider's own, so it can be added, and it is the owner's account's alone: its `modes` are `human-reader`
  alone, and only the owner's own words, a send naming that voice, go into it. A send in any other voice, or naming none, is refused `platform`
  with the policy's `read_only`, whatever its verb, so nothing answers in such a room.
- **A room the bot is not in is read through the owner's account only where the receiving side asks for it.**
  Other bots' words are read wherever the room is listened to, as in any room. A person's words there are
  read and handed over, as `live` in `human-reader`, only where the receiving side asked for the room to be
  read, so the owner's other chats never flow. Both sides of such a room are handed over: the owner's own
  words there, typed wherever the owner typed them, go as the owner's, under `BMO_OWNER_ID`, but for the
  messages a send in the owner's voice went out as, which the sender holds already (`docs/reader.md`, 「What
  it is for」). The request is the `read` of the room as the receiving side answers it; whether the room is
  listened to is no such request, and a receiving side that does not answer, or holds nothing of the room,
  asks for nothing.
- **A hand-over from a room the bot is not in carries no `identity`** in its room facts, as the room's own
  answer carries none.
- **A room's `modes`** are the voices the declaration has active in a room of the bot's, and `human-reader`
  alone in a room the bot is not in. Its `identity` is the bot's number in a room of the bot's, and null in one
  the bot is not in, since no words of the provider's own stand there but the owner's.
- **The list** holds the bot's rooms, the owner's one-to-one and every room seen that the bot is in, and every
  chat of the owner's account (`docs/reader.md`, 「The rooms of the account」), each room once: a chat the bot is
  also in is the bot's, under the account's title for it, and a room of the bot's the account does not list is
  titled as Telegram tells the bot. Each is shaped as `GET /rooms/{roomId}` answers it: a one-to-one, with a
  person or a bot, carries that one's number as `privateTo`, and every other chat, group, supergroup or channel,
  is `group` with no `privateTo`. Whether the bot is in each room is asked of Telegram eight rooms at a time,
  each answer kept as above, so walking the pages of a long list asks Telegram about each room once.

### Contacts and starting a conversation

- **The contacts are the owner's account's,** its Telegram contact list as the reader reads it
  (`docs/reader.md`, 「Contacts and starting a conversation」): each under the person's own number, with the
  name the account shows, `bot` for a bot account and `person` for any other, and as `roomId` the one-to-one the
  account already has with the contact, numbered as the service numbers rooms, or null. The bot account has no
  contacts.
- **The bot account cannot start a one-to-one.** Telegram lets a bot speak to a person only once the person has
  spoken to it, so every conversation is started by the owner's account, through the reader.
- **A one-to-one started is an owner-account room, read-only,** as any room the bot is not in (「The rooms」):
  `human-reader` alone and no `identity`, unless the bot is already in a one-to-one numbered the same, which is
  then the bot's. Starting it sends nothing: Telegram makes a one-to-one stand only once a message is in it, so
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

## Reading a room

- **The bot account long-polls the Bot API.** It first asks who it is, asking again until Telegram answers,
  so a message of its own is known by its number; until then the number is read off its token. Updates are
  read one at a time, in order; one that fails is said and the next is read.
- **The rooms the bot account listens to** are the owner's one-to-one and every room whose number the chat
  protocol carries whole, within the integers JavaScript counts exactly. A message from any other room has its
  words handed over and nothing else: no file, no picture and no expression is gathered.
- **What a message carries beyond words** is handed over as a placeholder in the text, the platform's English
  name of the kind with its particulars (a photo, a location, a poll, a sticker with its emoji and set, a
  forwarded message's origin, a member joining or leaving, a pin, a call), with the words typed under it as
  the context's `spokenText`. The placeholders are this repository's policy (`policy/telegram.toml`).
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
  says `gathering`, and the assets follow as `gathered`.
- **A file past the Bot API's ceiling for a bot,** picture or other, is fetched through the reader; with no
  reader standing, a picture goes without its bytes and another file is not kept.
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
- **A group the bot leaves** is handed over as the service message Telegram sends for it; a group moving to a
  supergroup's number is no message, and the reading goes on.
- **The bot's own messages** are not read back in: a file it sent is the sender's to keep, and a pin it made
  is nobody's word.
- **A message a send in the owner's voice went out as** is not read back in either: the sender holds it
  already, so the bot account's sight of it is that send's echo (「The owner's voice」).

## Sends

Every send is one verb, answered with the message it went out as or acted on. A send Telegram turns down for
what it asked — any answer of its in the 400s but its flood limit — is refused with the platform's own English
reason; a send about a message Telegram no longer has, or will never let change again, is refused and marked
`gone`. A send Telegram could not answer at all — no answer, its flood limit past what is waited out, or its
own fault — is the send's failure, in the protocol's error shape, and is never offered again.

| Verb | What goes out |
|---|---|
| `text` | the markdown in the dialect's markup, in as many messages as it takes, the first answering `replyTo`; folded where the send asks for a fold; without a notification where `silent`; plain text where Telegram refuses the markup. A part that does not go ends the send there |
| `photo` | a picture by web address, or by a file path under the picture root, tried by address first and uploaded where Telegram would not fetch it, redrawn for upload where Telegram would not take it as it is; the caption in the dialect's markup, and plain where Telegram refuses the markup. A picture that does not go is refused as `no-picture-sent`, its caption and web address sent as text in its place |
| `album` | pictures by address or handed over as files, up to ten to an album, a longer list split evenly so that no album holds one; the caption and `replyTo` with the first album; one picture left goes as a `photo`. Each picture that did not go is a `missed` picture with why, and an album none of which went is said as its caption and web addresses; where none of them went at all, the send is refused as `no-picture-sent` |
| `document` | a file with its name |
| `audio` | a sound file with its name, under the type its first bytes read as |
| `voice` | a voice note, the first part of the reading its caption as an expandable quote and the rest quiet replies under it, or its own caption where there is no reading; where Telegram refuses the caption, the voice goes again bare and the reading follows as messages under it. A reading that did not all go is the send's failure, naming the voice that went |
| `sticker` | a sticker of the configured set that carries the emoji, picked at random among them; refused where the set carries none |
| `reaction` | the emoji on the message; an empty emoji takes the reaction off |
| `pin` | the message pinned, quietly where `silent`, or unpinned |
| `typing` | the typing mark; taking it off asks Telegram nothing |
| `edit` | the message's words; an edit to the words the message already says is no failure |
| `delete` | the message |

- **A file or a picture answering `replyTo`** still goes where Telegram no longer has the message it answers;
  so does a text, unpinned.
- **Flood limits** are waited out as Telegram asks, never past a minute: an album and each part of a voice's
  reading are asked again twice at most.
- **A picture named by anything but a web address or a file under the picture root** is not sent: it is
  missed as `not-an-address`, `outside-data-root` or `missing-file`. With no picture root configured, no file
  path is sent. The file and the root are both resolved to where they really stand before either is read, so a
  symbolic link inside the root that reaches a file outside it is `outside-data-root`.
- **What went is not kept here.** Keeping a copy of a send is the sender's, under the number the result gives.
  The provider notes only, for a few minutes, the messages a send in the owner's voice went out as, to know
  their echo (「The owner's voice」).

### Refusals

Every refusal is one of bmo-chat's refusal codes under the label `Telegram`. The provider's own reasons are
English lines of `policy/telegram.toml`, each the detail of a `platform` refusal:

| When | Code | Detail |
|---|---|---|
| an act on a message Telegram no longer has, or will never let change again | `platform`, marked `gone` | `message_gone`, with Telegram's own description |
| a sticker the configured set does not carry | `platform` | `sticker_missing` |
| a sticker where no set is configured | `platform` | `sticker_unset` |
| a send asked to be spoken in a mode that is not running, the owner's voice while the reader does not have the account connected among them, or one the provider does not read by | `platform` | `mode_down`, with the mode's reason or `mode_missing` |
| a send in the owner's own voice of a verb other than `text`, `photo`, `album`, `document` and `audio` | `platform` | `owner_verb` |
| a send in any voice but the owner's own into a room the bot is not in (「The rooms」) | `platform` | `read_only` |
| any other refusal Telegram gives | `platform` | Telegram's own description |
| every picture of a send missed | `no-picture-sent` | each picture with its reason, below |

A picture that did not go is missed with one of bmo-chat's reasons: `not-an-address`, `outside-data-root`,
`missing-file` and `unfetched` carry the base's own sentences; a picture Telegram refused, one whose last try
failed and one left out of an album that went are `platform`, with the policy's `refused`, `failed` or
`left_out`. A provider that is not configured is answered `not-ready` by the base before it is asked.

### The owner's voice

A send is the owner's own words when it names the voice `human-reader`, as the chat protocol names a send in the
owner's own words; a send carries no sender of its own. Such a send goes out in the owner's own account,
through the reader, so the room reads it as the owner's, never as the bot's. A send that names no voice, or
`bot-account`, goes as the bot's.

- **The same message the bot's send makes.** The owner's account sends what the bot account would have sent
  for the same send, in the owner's name:
  - **`text`**: the markdown in the parts the dialect divides it into, each in the dialect's markup, folded
    where the send asks for a fold, and as its plain text where Telegram refuses the markup; without a
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
  - Any other verb is refused with the policy's `owner_verb`.
- **A reply** is named by the send's `replyToMessageId`, the message's number as the protocol carries it. The
  reader finds that number's line on the owner's account and answers it; where the account has no line under
  that number, the send is refused `unreachable` rather than sent without the reference the caller asked for.
- **While the reader does not have the account connected**, or no reader stands behind the service, the send
  is refused `platform` with the policy's `mode_down` and the reader's reason. It is never sent by the bot
  instead.
- **The number a send answers** is the one the owner's account gave the first message it went out as. In a
  supergroup or a channel that is the number everyone knows the message by; in any other room each account
  numbers its own messages, and the bot account knows the message by another.
- **Its echo is not handed over.** The bot account sees in its rooms what the owner's account sends there, as
  a message from the owner's number, `BMO_OWNER_ID`. Each message a send in the owner's voice went out as is
  noted, in memory and for five minutes, by its room, the number the owner's account gave it and the words
  Telegram holds for it. A message the bot account reads from the owner's number that answers a note is that
  send's echo: the note is let go and the message is not handed over, since the sender holds it already. In a
  supergroup or a channel the number answers the note; in any other room the words do, the oldest note first.
  A message from the owner that comes while a send in the owner's voice into its room is still going waits for
  that send, ten seconds at most, before it is matched.

## The dialect: `telegram-html`

The agent's markdown becomes Telegram's HTML. The dialect has:

| Member | Value |
|---|---|
| `limits` | text 4096, caption 1024 (both counted in UTF-16 units), album 10 pictures, file 20,000,000 bytes |
| `writing` | "This room shows formatting. Write markdown as the answer-shape rulebook describes: the send path turns it into this platform's own markup and escapes what has to be escaped, so never write a tag yourself. A long answer may be folded, which the formatting gate decides." |
| `foldable` | true: a long answer folds into an expandable quote |
| `convert` | markdown in, the HTML markup, the same words plain, and whether the markup says more than the plain text |
| `parts` | the markdown divided at block boundaries into pieces each of which fits one message |
| `fault` | an English reason where the markup is not sound HTML Telegram reads: a character left unescaped, a tag Telegram does not take, a tag left open, one closed that was never opened, two crossed, one nested where it may not be |
| `fold` | the folded markup, or null where there is nothing worth folding |
| `visible` | what a reader sees of the markup |

Whether a reply folds is the sender's decision, carried to the provider as the send's `fold`, as the fixed
protocol states it. The provider applies the dialect's fold where the send asks for one and asks no output gate
of its own.
