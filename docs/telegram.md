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
| `speakers` | false: every send goes out as the one bot account |

### Modes

| Voice | Reach | Standing | `canReplyTo` |
|---|---|---|---|
| `bot-account` | `all-messages` | `active` | true |
| `human-reader` | `all-messages` | `active` while the reader is connected; `inactive` otherwise, with the reader's reason, `BMO_TG_API_ID, BMO_TG_API_HASH, BMO_TG_SESSION not all set` where they are missing | false |

- **The bot account reads every message of every room it is in,** and is what the provider runs on with
  nothing else configured.
- **The owner's account is read only.** It sees the other bots' words, which the Bot API never shows a bot,
  and the bytes behind a picture or a file past the Bot API's ceiling. An answer cannot be pinned to a message
  it read, since the bot account is not the one that saw it.

### Capabilities

All twelve send verbs, and `replyTo`, `fetchMedia`, `avatars` and `history`. The last three are the owner's
account's: each is declared only while the reader mode is active, and `history` and `avatars` only while their
part stands (`GET /status`).

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
  path is sent.
- **What went is not kept here.** Keeping a copy of a send is the sender's, under the number the result gives.

### Refusals

Every refusal is one of bmo-chat's refusal codes under the label `Telegram`. The provider's own reasons are
English lines of `policy/telegram.toml`, each the detail of a `platform` refusal:

| When | Code | Detail |
|---|---|---|
| an act on a message Telegram no longer has, or will never let change again | `platform`, marked `gone` | `message_gone`, with Telegram's own description |
| a sticker the configured set does not carry | `platform` | `sticker_missing` |
| a sticker where no set is configured | `platform` | `sticker_unset` |
| a send asked to be spoken in a mode that is not running, or one the provider does not read by | `platform` | `mode_down`, with the mode's reason or `mode_missing` |
| a send in the owner's own voice of a verb other than `text` and `album` | `platform` | `owner_verb` |
| any other refusal Telegram gives | `platform` | Telegram's own description |
| every picture of a send missed | `no-picture-sent` | each picture with its reason, below |

A picture that did not go is missed with one of bmo-chat's reasons: `not-an-address`, `outside-data-root`,
`missing-file` and `unfetched` carry the base's own sentences; a picture Telegram refused, one whose last try
failed and one left out of an album that went are `platform`, with the policy's `refused`, `failed` or
`left_out`. A provider that is not configured is answered `not-ready` by the base before it is asked.

### The owner's voice

A send with `as` `human-reader` goes out in the owner's own account, through the reader. It takes two verbs:

- **`text`**: the owner's words as they stand, cut into the pieces Telegram takes, each after the last pause
  that fits, and a run with no pause between whole characters.
- **`album`**: files go as the albums Telegram takes: pictures and videos together, documents together,
  sounds together, a GIF alone, each album up to Telegram's ceiling, in the order their first file came. An
  item named by an address is read as a picture of the bot's is. The caption goes with the first album, its
  line breaks each a single line feed and its ends trimmed. A file that holds nothing, or one past the
  `[owner]` ceiling, is missed with the policy's `empty_file` or `too_large`; an album the account refuses is
  missed by every file in it, and stops none after it.
- **Neither answers `replyTo`:** the owner's account finds a message by when it was said and what it says,
  which the protocol's send does not carry.
- **While the reader does not have the account connected**, the send is refused with the reader's reason.

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
