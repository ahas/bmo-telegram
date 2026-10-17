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
| `reader/` | the reader, a bun package holding the owner's account over MTProto, with its tests in `reader/tests/` |
| `policy/` | the provider's fixed numbers and English words: its limits, refusals and placeholders |
| `tests/` | the checks that span the repository, and the service's tests |
| `docs/` | the canonical statements of what the provider does |

At the root stand the Cargo workspace manifest, which is also the service's package, the version in
`VERSION`, `testbed.toml` (the testbed's declaration of how this repository is tested, checked, built and
deployed), `rustfmt.toml` and `CLAUDE.md`.

## Documents

| Document | States |
|---|---|
| `docs/boundary-api.md` | The boundary: the routes served, the calls made on the receiver and what is needed of it, the seam between the service and the reader, the configuration |
| `docs/telegram.md` | The provider: its declaration, how it reads a room, what each send does, the owner's voice, the `telegram-html` dialect |
| `docs/reader.md` | The reader: the owner's account, connecting, the history walk, profile pictures, and why it is TypeScript |
| `docs/checks.md` | The repository checks: dependencies, manifests, layout, the version, the testbed's declaration, Hangul, home paths, private addresses and names |

## Version

- **One version for the whole repository,** the workspace's, in semver. `VERSION` holds it, the workspace's
  version equals it, and the reader's `package.json` carries it.
- **A major change is a change to a contract a consumer reads;** an addition no existing consumer reads is a
  minor change, anything else a patch.

## Build and test

Cargo runs through the testbed's Cargo verb, which builds in the one shared build directory:

```sh
bmo-testbed cargo check --workspace --all-targets
bmo-testbed cargo test --workspace
bmo-testbed cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The reader's tests run in `reader/`:

```sh
bun install
bun test
```
