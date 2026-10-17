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
  so a message of its own is known by its number; until then the number is read off its token.
- **A message from a room the receiver does not listen to** has its words handed over and nothing else: no
  file, no picture and no expression is gathered (`docs/boundary-api.md`, 「Needed from the receiver」).
- **What a message carries beyond words** is handed over as a placeholder in the text, the platform's English
  name of the kind with its particulars (a photo, a location, a poll, a sticker with its emoji and set, a
  forwarded message's origin, a member joining or leaving, a pin, a call), with the words typed under it as
  the context's `spokenText`. The placeholders are this repository's policy (`policy/telegram.toml`).
- **A file is gathered through the Bot API** and handed over as bytes: on the attachment where it is gathered
  at once, or as `gathering` followed by `gathered`. A file past the Bot API's ceiling for a bot is fetched
  through the reader. A file that could not be kept is handed over as `not-kept` with the reason.
- **Pictures are kept as Telegram's largest size,** and a picture sent as a file is still a picture; an SVG is
  a file. A picture past the keeping threshold, or one with transparency, is brought inside the keeping
  dimension, laid on white and written as JPEG.
- **A send of several messages,** an album, is named by the first of its messages the provider read, in its
  own room, and every message of it carries that number as `album`.
- **An edit** is handed over as `edited` with the words, the formatting and the file in the order Telegram
  wrote them. Telegram's own payload crosses as `metadata`, its keys in Telegram's order.
- **A group the bot leaves, or one that moves to a supergroup's number,** is handed over as the service
  message Telegram sends for it, so the receiver keeps every record of the room under its numbers.
- **The bot's own messages** are gathered only: the file is kept for the receiver, and the words are not read
  back in as somebody else's.

## Sends

Every send is one verb. A send of a verb Telegram does not take for that message is refused with the
platform's own English reason; a send about a message Telegram no longer has, or will never let change again,
is refused and marked `gone`.

| Verb | What goes out |
|---|---|
| `text` | the markdown in the dialect's markup, in as many messages as it takes; folded where the send asks for a fold; without a notification where `silent`; plain text where Telegram refuses the markup |
| `photo` | a picture by web address, or by a file path under the picture root, redrawn for upload where Telegram would not take it as it is; the caption in the dialect's markup |
| `album` | up to ten pictures to an album, a longer list split evenly so that no album holds one; each picture that did not go is a `missed` picture with why |
| `document` | a file with its name |
| `audio` | a sound file with its name |
| `voice` | a voice note, its caption the reading; where Telegram refuses the caption, the voice goes again bare and the reading follows as messages under it |
| `sticker` | the sticker of the configured set that carries the emoji; refused where the set carries none |
| `reaction` | the emoji on the message; an empty emoji takes the reaction off |
| `pin` | the message pinned, quietly where `silent`, or unpinned |
| `typing` | the typing mark |
| `edit` | the message's words; an edit to the words the message already says is no failure |
| `delete` | the message |

- **Flood limits** are waited out as Telegram asks, never past a minute.
- **A picture named by anything but a web address or a file under the picture root** is not sent: it is
  missed as `not-an-address`, `outside-data-root` or `missing-file`.

### Refusals

Every refusal is one of bmo-chat's refusal codes under the label `Telegram`. The provider's own reasons are
English lines of `policy/telegram.toml`, each the detail of a `platform` refusal:

| When | Code | Detail |
|---|---|---|
| an act on a message Telegram no longer has, or will never let change again | `platform`, marked `gone` | `message_gone`, with Telegram's own description |
| a sticker the configured set does not carry | `platform` | `sticker_missing` |
| a sticker where no set is configured | `platform` | `sticker_unset` |
| a send asked to be spoken in a mode that is not running, or one the provider does not read by | `platform` | `mode_down`, with the mode's reason or `mode_missing` |
| any other refusal Telegram gives | `platform` | Telegram's own description |
| every picture of a send missed | `no-picture-sent` | each picture with its reason, below |

A picture that did not go is missed with one of bmo-chat's reasons: `not-an-address`, `outside-data-root`,
`missing-file` and `unfetched` carry the base's own sentences; a picture Telegram refused, one whose last try
failed and one left out of an album that went are `platform`, with the policy's `refused`, `failed` or
`left_out`. A provider that is not configured is answered `not-ready` by the base before it is asked.

### The owner's voice

A send with `as` `human-reader` goes out in the owner's own account, through the reader:

- **Words** are cut into the pieces Telegram takes, each after the last pause that fits, and a run with no
  pause between whole characters.
- **Files** go as the albums Telegram takes: pictures and videos together, documents together, sounds
  together, a GIF alone, each album up to Telegram's ceiling, in the order their first file came.
- **A voice the reader does not have connected** is refused with the reader's reason.

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
