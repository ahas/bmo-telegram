# Working in this repository

This repository is the Telegram chat provider, `bmo-telegram`: Telegram as the receiver sees every chat
service, through bmo-chat's provider contract. The service at the root holds the bot account's connection to
the Bot API, declares the provider's modes, capabilities and dialect, performs the send verbs and hands what
its rooms say to the receiver. The owner's own account is the reader's, a bun process in `reader/` that only
the service calls, over the loopback.

## Rules

- **`docs/` is canonical.** Read the document covering a change before changing code. A change that would
  make it wrong or incomplete repairs it first, stating what is true now, and the code is then written to
  match. `docs/boundary-api.md` states every route this provider serves, every call it makes on another unit,
  the seam between its two processes and the configuration it reads.
- **The code graph in `.graph/` is written only by `bmo graph`.** It holds one node per feature with its
  files, exported names, edges and boundary; nothing else edits it, and it is committed with the code it
  names.
- **No `bmo-*` dependency but the base libraries** `bmo-chat`, `bmo-chat-client` and `bmo-ui`. Another unit is
  reached over HTTP, as the boundary states; a helper an old shared crate held is copied into a crate here.
  The reader depends on no `@bmo/` package. `tests/repository.rs` fails on either (`docs/checks.md`).
- **The provider declares what it is.** Its modes, capabilities and dialect are its own declaration; nothing
  here names the receiver's internals or another provider, and nothing branches on an agent's name.
- **English only** in code, comments, strings, tests, policy, documents and commit messages. A refusal is a
  bmo-chat refusal code with an English sentence; a placeholder for what a message carries is English and
  lives in `policy/telegram.toml`. No tracked file holds Hangul but the fixed marker at the top of
  `docs/api.md`.
- **No agent names, owner names, host names, home paths or private network addresses** in code, documents or
  commits. A deployment value that would name one is configuration (`docs/boundary-api.md`,
  「Configuration」); tests use neutral names. No list of forbidden names is kept in the repository; the names
  check reads one from outside it (`docs/checks.md`).
- **No personal data in a tracked file.** `docs/PERSONAL-DATA.md` states the rule and `tests/personal_data.rs`
  holds it: no mail address, home path, telephone number, token or private network address. The check reports
  the file, the line and the kind, never the value.
- **Tests live in `tests/`.** A crate's tests are in its own `tests/`; checks that span the repository and the
  service's own tests are in the root `tests/`; the reader's are in `reader/tests/`. No test module sits in
  `src/`; a private function a test needs becomes public in its own module.
- **No directory named after its parent** (no `x/x`), and **only Cargo manifests at the root**: the root
  `Cargo.toml` is the workspace and also the service's package. The reader's `package.json` is in `reader/`.
- **Documents state what is true now**: no change history, no quoted source and no file-and-line pins.

## Checks

A change moves through three stages in order:

1. Writing: `cargo check --workspace --all-targets`, and in `reader/` `bunx tsc --noEmit` once it has
   TypeScript.
2. Review: read the whole diff against what the change is for and the document governing it, then
   `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` (108 columns,
   2-space indentation, as `rustfmt.toml` sets).
3. Verification: the test targets the change touched, then `cargo test --workspace` and `bun test` in
   `reader/` once, the suites `testbed.toml` declares.

Cargo runs through the testbed's Cargo verb, `bmo-testbed cargo <cargo arguments…>`, which builds in the one
shared build directory; no checkout keeps a `target/` of its own. Where the installed testbed predates that
verb and runs `cargo` as a plain command, `bmo cargo <cargo arguments…>` carries the same verb.

The version is the workspace's, in semver, and `VERSION` holds it: a change to a contract a consumer reads
moves the major version, an addition no existing consumer reads the minor, anything else the patch. The
reader's `package.json` carries the same version.
