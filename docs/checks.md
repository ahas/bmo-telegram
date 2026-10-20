# The repository checks

`tests/repository.rs` holds the checks that span the repository. Each runs with the root package's tests.

- **Dependencies.** No package of the workspace depends on a `bmo-*` package but the base libraries,
  `bmo-chat`, `bmo-chat-client` and `bmo-ui`, nor on a crate named after a shared crate of the single
  repository this one was separated from (`archive-pool`, `archive-read`, `bmo-lang`, `bmo-markup`,
  `bmo-picture`, `bmo-policy`, `bmo-tool`, `room-acts`, `seoul-clock`, `harness-providers`, `agent-host`,
  `test-support`). No package of the workspace takes one of those names either. What such a crate held that
  this provider uses lives in a crate of this repository under a name of its own.
- **The reader's dependencies.** `reader/package.json` names no `@bmo/` package and no workspace or file
  dependency: the reader stands on its own.
- **Manifests.** Only Cargo manifests stand at the root. The one other build tool's manifest in the
  repository is the reader's, in `reader/`.
- **Layout.** No tracked path holds a directory named after the directory it stands in.
- **The version.** `VERSION` holds the workspace's version, and the reader's `package.json` carries it.
- **The testbed's declaration.** `testbed.toml` names only keys of the testbed's format, its major version,
  the Cargo and bun suites and the three base libraries.
- **Hangul.** No tracked file holds a Hangul character but the fixed marker at the top of `docs/api.md`, which
  is written as the fixed-API rule states it: every other word this provider writes or matches on is English.
- **Home paths and private addresses.** No tracked file holds a path under a home directory or an address of a
  private network range.
- **Names.** No tracked file holds an agent's, a person's or a machine's name. No repository keeps the list of
  those names, so the check reads it from the file the variable `BMO_NAMES_LIST` names, outside the checkout:
  a TOML file with `words` (matched as whole words, ignoring case), `exact_words` (whole words, case kept),
  `anywhere` (matched anywhere) and `allowed` (terms of another unit's protocol that hold a name, set aside
  first). Where the variable names no file, the names are not checked and the run says so.
