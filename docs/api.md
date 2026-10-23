# The Telegram provider's fixed API

**고정됨 0.7.0.**

This service is a chat provider, so its public API is the chat protocol's provider routes, fixed by bmo-chat at
0.1.0-rev.6 (the release its history numbers 1.0.0). The service serves them through bmo-chat's provider server
and answers them itself; it adds no public route of its own. `docs/openapi.json` is the same contract in machine
form, exactly as bmo-chat-plugin generates it from the published types, and the routes below are that document's.

The Rust crate this service builds on, `bmo-chat`, and the receiving side's client, `bmo-chat-client`, are the
protocol's published pair. This repository writes no client of its own for another unit: the receiver is reached
through `bmo-chat`'s link, and no hand-written call module of an upstream route stands here.

The seam between the service and the reader, and the service's own `/status`, are not public: the reader is this
repository's own process, called over the loopback (`docs/boundary-api.md`, 「The seam between the service and
the reader」).

## The provider

| Method | Path | Answer |
|---|---|---|
| `GET` | `/declaration` | the provider's declaration: its key, label, modes, capabilities, dialect, numbering and speakers |
| `GET` | `/health` | `ready`, `reason` and the service's `version` |

## Rooms

| Method | Path | Answer |
|---|---|---|
| `GET` | `/rooms` | a page of the rooms the bot account and the owner's account are in, each once, in ascending order of `id`, walked by `after` (a room) and `limit`, each as `GET /rooms/{roomId}` shapes a room; refused `unsupported-capability` while `roomList` is not declared |
| `GET` | `/rooms/{roomId}` | what the provider knows of the room: `id`, `owns`, `identity` (null in a room the bot is not in, which takes only the owner's own words), `kind`, `privateTo` and `title`; 404 `not-found` where it knows nothing of it |
| `GET` | `/owner` | `roomId`, the room to reach the owner in, or null |

## Sends

| Method | Path | Answer |
|---|---|---|
| `POST` | `/sends` | 201, the send's `messageId` and its `missed` pictures |

A send is one `kind` and its `fields`, with an optional `mode`, `speaker`, `tag`, `prefix` and
`replyToMessageId`. A refused send is a non-2xx answer in the error shape.

## Files

| Method | Path | Answer |
|---|---|---|
| `GET` | `/rooms/{roomId}/messages/{messageId}/files` | a page of the message files a message carried, fetched again, each with `id`, `name`, `mime`, `bytes` and its `kind` where known; the query takes an optional `sentAt` |

## The dialect

| Method | Path | Answer |
|---|---|---|
| `POST` | `/dialect/readings` | the `telegram-html` dialect's reading of markdown: `markup`, `plain`, `formatted`, `parts` and `fold` |
| `POST` | `/dialect/checks` | the dialect's `fault` in markup it built, and its `visible` reading |

## Profile pictures

| Method | Path | Answer |
|---|---|---|
| `GET` | `/avatars` | a page of the pictures kept, each `kind`, `id` and `takenAt`, oldest taken first, walked by `after` and `limit` |
| `GET` | `/rooms/{roomId}/avatar` | a room's kept picture, as its bytes, typed by `Content-Type`; 404 `not-found` where none is kept |
| `GET` | `/people/{personId}/avatar` | a person's kept picture, as its bytes; 404 `not-found` where none is kept |

## The wire

- **Keys** are camelCase. **Times** are integers of epoch milliseconds. **Rooms, people and messages** are signed
  64-bit integers. A message file's id is an opaque string, the same each time the same file crosses and never
  shared by two files of one message.
- **A list is a page**, `{"items": [...], "more": boolean}`. The room list is walked by `after` (a room) and
  `limit`, the avatar list by `after` (a time) and `limit`; a page never ends between two pictures taken at the
  same moment. A message's files are one page.
- **A failure is a non-2xx status** whose body is `{"code": ..., "error": ...}`. A refusal is answered by status
  too; no 2xx carries a refusal. The codes are the protocol's: `bad-request`, `unauthorized`, `no-route`,
  `not-found`, `conflict`, `gone`, `failed`, `not-ready`, `unavailable`, `unsupported-capability`,
  `no-picture-sent`, `speaker-not-allowed`, `speaker-unnumbered` and `platform`.
- **A profile picture** is the one answer that is not JSON: its bytes, typed by `Content-Type`. Where none is
  kept, the answer is 404 `not-found`.
- **Auth.** Every route takes `Authorization: Bearer <token>` with the service's token and answers 401 without
  it.
- **No caller is named** in a route, a body or a section. Sends and fetches answer every caller the same way.
- **Retrying.** A caller offers a request again only when the connection was refused. A request that reached the
  service is never made twice; a caller answered `not-ready` decides itself whether to ask again after
  `Retry-After`.

The full bodies, limits and refusal sentences are bmo-chat's `docs/protocol.md` and `docs/contract.md`; this
document states what is this service's, and defers to them where the protocol speaks.
