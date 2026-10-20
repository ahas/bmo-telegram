//! No tracked file holds personal data (docs/PERSONAL-DATA.md): no mail address, home path, telephone
//! number, token or private network address. The check names nobody: it matches shapes and reports the
//! file, the line and the kind, never the value it found.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use regex::Regex;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Home folders that are role names rather than a person's, so a neutral example.
const NEUTRAL_HOME: [&str; 6] = ["user", "pc", "tester", "qa-admin", "placeholder", "example"];

/// Domains reserved for documentation and testing, where a neutral example address stands.
const EXAMPLE_DOMAINS: [&str; 3] = ["example.com", "example.net", "example.org"];

/// Domain suffixes reserved for documentation and testing.
const EXAMPLE_SUFFIXES: [&str; 4] = [".example", ".test", ".invalid", ".localhost"];

/// What a neutral value may be for one of the forms.
enum Allowance {
  /// Every match is forbidden.
  None,
  /// A match is allowed at a reserved example domain.
  Mail,
  /// A match is allowed when its home folder is a role name.
  Home,
  /// An assigned value must carry a digit; a vendor's key shape always counts.
  Token,
}

/// One form a tracked file may not hold, and what a failure calls it.
struct Sought {
  kind: &'static str,
  pattern: Regex,
  allowance: Allowance,
}

impl Sought {
  /// Whether `line` holds this form somewhere the neutral values do not cover.
  fn holds(&self, line: &str) -> bool {
    match self.allowance {
      Allowance::None => self.pattern.is_match(line),
      Allowance::Mail => self.pattern.find_iter(line).any(|found| !is_example_mail(found.as_str())),
      Allowance::Home => self.pattern.captures_iter(line).any(|capture| !is_neutral_home(&capture[1])),
      Allowance::Token => self.pattern.captures_iter(line).any(|capture| match capture.get(1) {
        Some(value) => value.as_str().chars().any(|character| character.is_ascii_digit()),
        None => true,
      }),
    }
  }
}

/// A home directory as an account's is written: the folder followed by a name, on a Unix-like path or a
/// Windows one.
fn home_path() -> Sought {
  Sought {
    kind: "a home path",
    pattern: Regex::new(r"(?:/home/|/Users/|/root/|[A-Za-z]:\\Users\\)([A-Za-z0-9._-]+)")
      .expect("the home path builds"),
    allowance: Allowance::Home,
  }
}

/// A mail address.
fn mail_address() -> Sought {
  Sought {
    kind: "a mail address",
    pattern: Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
      .expect("the mail address builds"),
    allowance: Allowance::Mail,
  }
}

/// A telephone number written in groups, with an optional country or area code.
fn telephone_number() -> Sought {
  Sought {
    kind: "a telephone number",
    pattern: Regex::new(r"(?:\+\d{1,3}[ .-]?)?(?:\(\d{1,4}\)[ .-]?)?\d{2,4}[ .-]\d{3,4}[ .-]\d{4}\b")
      .expect("the telephone number builds"),
    allowance: Allowance::None,
  }
}

/// A token: a long value carrying a digit assigned to a secret's name, or the shape a vendor gives its
/// keys.
fn token() -> Sought {
  let pattern = concat!(
    r#"(?i)(?:\b(?:api[_-]?key|access[_-]?token|refresh[_-]?token|auth[_-]?token|token|secret|key|"#,
    r#"client[_-]?secret|password|passwd|bearer)\b\s*[:=]\s*["']?([A-Za-z0-9_+/=.-]{16,})"#,
    r"|\bgh[pousr]_[A-Za-z0-9]{20,}|\bgithub_pat_[A-Za-z0-9_]{20,}",
    r"|\bsk-[A-Za-z0-9_-]{20,}|\bxox[abprs]-[A-Za-z0-9-]{10,}",
    r"|\bAKIA[0-9A-Z]{16}\b|\bAIza[0-9A-Za-z_-]{35}\b",
    r"|\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})"
  );
  Sought {
    kind: "a token",
    pattern: Regex::new(pattern).expect("the token builds"),
    allowance: Allowance::Token,
  }
}

/// A private or link-local network address. A loopback address is neither and passes.
fn private_address() -> Sought {
  let pattern = concat!(
    r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}",
    r"|172\.(?:1[6-9]|2[0-9]|3[01])\.\d{1,3}\.\d{1,3}",
    r"|192\.168\.\d{1,3}\.\d{1,3}",
    r"|100\.(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.\d{1,3}\.\d{1,3}",
    r"|169\.254\.\d{1,3}\.\d{1,3})\b",
    r"|\b(?:f[cd][0-9a-f]{2}|fe80)(?::[0-9a-f]{0,4}){2,}\b"
  );
  Sought {
    kind: "a private network address",
    pattern: Regex::new(pattern).expect("the private address builds"),
    allowance: Allowance::None,
  }
}

/// The forms no tracked file may hold (docs/PERSONAL-DATA.md).
fn forms() -> Vec<Sought> {
  vec![home_path(), mail_address(), telephone_number(), token(), private_address()]
}

/// Whether a mail address stands at a reserved example domain.
fn is_example_mail(address: &str) -> bool {
  address
    .rsplit_once('@')
    .is_some_and(|(_, domain)| is_example_domain(&domain.to_lowercase()))
}

fn is_example_domain(domain: &str) -> bool {
  EXAMPLE_DOMAINS
    .iter()
    .any(|example| domain == *example || domain.ends_with(&format!(".{example}")))
    || EXAMPLE_SUFFIXES.iter().any(|suffix| domain.ends_with(suffix))
}

/// Whether a home folder is a role name rather than a person's.
fn is_neutral_home(folder: &str) -> bool {
  NEUTRAL_HOME.contains(&folder)
}

/// Every file git tracks or would track: the committed ones and the new ones it does not ignore.
fn tracked() -> Vec<String> {
  let output = Command::new("git")
    .args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
    .current_dir(ROOT)
    .stdin(Stdio::null())
    .output()
    .expect("git lists the files");
  assert!(
    output.status.success(),
    "git ls-files failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  String::from_utf8(output.stdout)
    .expect("the paths are text")
    .split('\0')
    .filter(|path| !path.is_empty() && PathBuf::from(ROOT).join(path).is_file())
    .map(str::to_owned)
    .collect()
}

/// Each line of `text` holding one of `forms`, as `path:line: kind`.
fn found_lines(path: &str, text: &str, forms: &[Sought]) -> Vec<String> {
  text
    .lines()
    .enumerate()
    .flat_map(|(number, line)| {
      forms
        .iter()
        .filter(|form| form.holds(line))
        .map(move |form| format!("{path}:{}: {}", number + 1, form.kind))
    })
    .collect()
}

#[test]
fn no_tracked_file_holds_personal_data() {
  let forms = forms();
  let found: Vec<String> = tracked()
    .into_iter()
    .flat_map(|path| {
      let bytes = fs::read(Path::new(ROOT).join(&path)).expect("a tracked file is readable");
      found_lines(&path, &String::from_utf8_lossy(&bytes), &forms)
    })
    .collect();
  assert!(found.is_empty(), "these lines hold personal data (docs/PERSONAL-DATA.md): {found:#?}");
}

#[test]
fn every_form_finds_its_own_and_passes_the_ordinary() {
  // Each sample is put together here, so this file holds no value of its own.
  let home = format!("/{}/{}", "home", "someone");
  let mail = format!("{}@{}.{}", "a.person", "mail", "notexample");
  let telephone = format!("{}-{}-{}", "010", "1234", "5678");
  let private = format!("{}.{}.{}.{}", 192, 168, 0, 10);
  let shared = format!("{}.{}.{}.{}", 100, 64, 0, 10);
  let forms = forms();
  let text = [
    format!("cd {home}/work"),
    format!("mail {mail}"),
    format!("call {telephone}"),
    format!("token = \"{}\"", "abcdefghijklmnop1234"),
    format!("serve on {private}"),
    format!("relay over {shared}"),
    format!("mail tester@{}.{}", "mail", "example.org"),
    format!("cd C:\\Users\\{}", "user"),
    "the loopback 127.0.0.1:7841 is ordinary".to_owned(),
    "version 0.6.1".to_owned(),
  ]
  .join("\n");
  assert_eq!(
    found_lines("sample", &text, &forms),
    [
      "sample:1: a home path",
      "sample:2: a mail address",
      "sample:3: a telephone number",
      "sample:4: a token",
      "sample:5: a private network address",
      "sample:6: a private network address",
    ]
  );
}
