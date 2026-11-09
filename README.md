# telegram

The Telegram chat provider, `bmo-telegram`. It is Telegram to the chat receiver, through the chat protocol of
the base library `bmo-chat`: it declares its read modes, what it can do and the dialect its rooms read, reads
its rooms through the bot account, sends the twelve verbs, and hands what its rooms say to the receiver. The
owner's own account, read only, stands behind it for what a bot cannot see or fetch and for the owner's own
words, held by a separate process, the reader.

## Layout

| Folder | Holds |
|---|---|
| `src/` | the service's own package: the provider, its Bot API connection and its routes |
| `dialect/` | `telegram-dialect`, the `telegram-html` dialect: markdown into Telegram's HTML, the structure check, the fold and the division into messages |
| `picture/` | `telegram-picture`, a picture read and redrawn: its type, its transparency, an upload Telegram takes as a photo, and a picture kept inside its size |
| `reader/` | the reader, a bun package holding the owner's account over MTProto, with its tests in `reader/tests/` |
| `tools/` | the hosted tool the deployment places: one chat's recent messages through the service, on the owner's word alone |
| `policy/` | the provider's fixed numbers and English words: its limits, refusals and placeholders |
| `tests/` | the checks that span the repository, and the service's tests |
| `deploy/` | the two service units a deployment links: the Rust provider and the bun reader |
| `docs/` | the canonical statements of what the provider does |
| `.graph/` | the repository's code graph: one node per feature with its files, symbols, edges and boundary, written only by `bmo graph` |

At the root stand the Cargo workspace manifest, which is also the service's package, the version in
`VERSION`, `testbed.toml` (the testbed's declaration of how this repository is tested, checked, built and
deployed), `rustfmt.toml` and `CLAUDE.md`.

## Documents

| Document | States |
|---|---|
| `docs/api.md` | The provider's fixed public API: the chat protocol's routes, its wire and its pages, and `docs/openapi.json`, the same contract in machine form |
| `docs/boundary-api.md` | The boundary: the routes served, the calls made on the receiver, the seam between the service and the reader, the configuration |
| `docs/telegram.md` | The provider: its declaration, how it reads a room, what each send does, the owner's voice, the `telegram-html` dialect |
| `docs/reader.md` | The reader: the owner's account, connecting, the history walk, profile pictures, and why it is TypeScript |
| `docs/tools.md` | The hosted tool: its entry, arguments, the owner's word, and where the provider is reached |
| `docs/checks.md` | The repository checks: dependencies, manifests, layout, the version, the testbed's declaration, Hangul, personal data and names |
| `docs/PERSONAL-DATA.md` | The repository's personal-data prohibition: what no tracked file may hold, what goes in its place, and the check that holds it |

## Version

- **One version for the whole repository,** the workspace's. `VERSION` holds it, the workspace's version
  equals it, and the reader's `package.json` carries it.
- **It moves only at a release the owner declares,** as bmo-chat's `docs/conventions.md` 「Versions」 states.
  A commit, a deploy, a fix or a contract change moves none, and a deployment is told apart by its commit.

## Build and test

Cargo runs through the testbed's Cargo verb, which builds in the one shared build directory; where the
installed testbed has no Cargo verb of its own, `bmo cargo` carries the same verb:

```sh
bmo cargo check --workspace --all-targets
bmo cargo test --workspace
bmo cargo clippy --workspace --all-targets -- -D warnings
bmo cargo fmt --all --check
```

The reader's tests run in `reader/`:

```sh
bun install
bun test
```
