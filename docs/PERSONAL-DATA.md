# Personal data

The repository holds nothing that belongs to a person, a machine or a network. A value that differs by
deployment belongs to that deployment's environment or arguments, and the repository keeps only a neutral
example.

## What no tracked file may hold

- A person's name, handle, account id or display name.
- A mail address.
- A telephone number.
- A home directory path.
- A host name, a domain name or a machine name.
- A device, a network address or another detail of a network.
- A company's name.
- A line of a real conversation.

## Where the rule applies

- Every file git tracks, and every new file git would track.
- Code, comments and strings.
- Documents and the README.
- Test fixtures and examples.
- Policy files and configuration examples.
- Commit messages, branch names and tags.
- Anything published from the repository.

## What goes in its place

- A value the service needs at runtime comes from its deployment's environment or arguments, never from the
  code.
- An example is neutral: a role such as `tester` or `qa-admin`, a domain reserved for documentation such as
  `example.org`, never a real name, account, address or a line someone actually wrote.
- A person, a machine or a network is named by its role, not by the name it carries.

## Neutral values the check lets through

- A mail address at a reserved example domain: `example.com`, `example.net` or `example.org`, one of their
  subdomains, or a domain ending in `.example`, `.test`, `.invalid` or `.localhost`.
- A home path whose account folder is a role name: `user`, `pc`, `tester`, `qa-admin`, `placeholder` or
  `example`.
- A loopback address, such as `127.0.0.1` or `::1`.

## The values the repository allows

None the check reads. `LICENSE` carries the copyright holder's name; the check matches shapes rather than
names, so a name is held by review.

## The check

The testbed's `personal-data` check, which the declaration enables, reads every tracked file, line by line,
before anything is built, and fails on:

- a mail address outside the domains kept for documents;
- a home path that names an account, on a Unix-like system or a Windows one, outside the neutral role names;
- a telephone number written in groups, with an optional country or area code;
- a token in the shape a vendor gives its own keys, a private key's opening line, or a bearer value;
- a private network address: the IPv4 blocks 10/8, 172.16/12, 192.168/16 and the shared 100.64/10, the IPv4
  link-local block 169.254/16, the IPv6 unique-local prefix fc00::/7 and the IPv6 link-local prefix
  fe80::/10. A loopback address passes.

The forms themselves are the testbed's policy (its `docs/checks.md`, 「The quick checks」).

A failure names the file, the line and the kind, never the value it found. The check keeps no list of
personal terms: it matches shapes. A rule the check cannot read, such as a name written into a commit
message or a host name written into a document, is held by review.
