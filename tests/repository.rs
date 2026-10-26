//! Checks that span the repository (docs/checks.md): what its packages and the reader may depend on, its
//! layout and manifests, its one version, the testbed's declaration, and what no tracked file may hold
//! (Hangul, and the names a list kept outside the repository gives). Personal data has its own check,
//! `personal_data.rs` (docs/PERSONAL-DATA.md).

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};
use std::process::Command;

use serde_json::Value;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The only `bmo-*` packages a package here may depend on: the base libraries.
const BASE_LIBRARIES: [&str; 3] = ["bmo-chat", "bmo-chat-client", "bmo-ui"];

/// Shared crates of the single repository this one was separated from; their parts live in-tree here.
const OLD_SHARED_CRATES: [&str; 12] = [
  "archive-pool",
  "archive-read",
  "bmo-lang",
  "bmo-markup",
  "bmo-picture",
  "bmo-policy",
  "bmo-tool",
  "room-acts",
  "seoul-clock",
  "harness-providers",
  "agent-host",
  "test-support",
];

/// File names that declare a package or a project to some other build tool.
const OTHER_MANIFESTS: [&str; 11] = [
  "package.json",
  "pyproject.toml",
  "setup.py",
  "go.mod",
  "deno.json",
  "bun.lock",
  "bun.lockb",
  "pnpm-workspace.yaml",
  "Gemfile",
  "pom.xml",
  "build.gradle",
];

/// The one folder another build tool's manifest may stand in: the owner's reader, a bun package.
const READER: &str = "reader";

/// The dependency tables of a package manifest.
const READER_DEPENDENCY_TABLES: [&str; 4] =
  ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"];

/// Version specifiers that reach into another checkout or workspace instead of a registry.
const LOCAL_SPECIFIERS: [&str; 4] = ["workspace:", "file:", "link:", "portal:"];

/// The variable naming the file of names no tracked file may hold: agents', people's and machines'. No
/// repository keeps such a list, so it is read from outside the checkout (docs/checks.md).
const NAMES_LIST: &str = "BMO_NAMES_LIST";

/// The Hangul blocks of Unicode: Jamo, compatibility Jamo, Jamo extended A and B, syllables, and the
/// halfwidth forms.
const HANGUL: [(u32, u32); 6] = [
  (0x1100, 0x11FF),
  (0x3130, 0x318F),
  (0xA960, 0xA97F),
  (0xAC00, 0xD7AF),
  (0xD7B0, 0xD7FF),
  (0xFFA0, 0xFFDC),
];

fn metadata() -> Value {
  let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
  let output = Command::new(cargo)
    .args(["metadata", "--no-deps", "--format-version", "1"])
    .current_dir(ROOT)
    .output()
    .expect("cargo metadata runs");
  assert!(
    output.status.success(),
    "cargo metadata failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  serde_json::from_slice(&output.stdout).expect("cargo metadata answers JSON")
}

/// Every file git tracks or would track: the committed ones and the new ones it does not ignore.
fn tracked_files() -> Vec<String> {
  let output = Command::new("git")
    .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
    .current_dir(ROOT)
    .output()
    .expect("git ls-files runs");
  assert!(
    output.status.success(),
    "git ls-files failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  let mut files: Vec<String> = String::from_utf8(output.stdout)
    .expect("tracked paths are UTF-8")
    .split('\0')
    .filter(|path| !path.is_empty())
    .map(str::to_owned)
    .collect();
  files.sort();
  files.dedup();
  files
}

/// The text of a tracked file, or nothing when it is gone from the work tree or is not UTF-8 text.
fn text_of(path: &str) -> Option<String> {
  match fs::read(Path::new(ROOT).join(path)) {
    Ok(bytes) => String::from_utf8(bytes).ok(),
    Err(error) if error.kind() == ErrorKind::NotFound => None,
    Err(error) => panic!("{path} is unreadable: {error}"),
  }
}

fn tracked_texts() -> Vec<(String, String)> {
  tracked_files()
    .into_iter()
    .filter_map(|path| text_of(&path).map(|text| (path, text)))
    .collect()
}

fn is_hangul(character: char) -> bool {
  let point = u32::from(character);
  HANGUL.iter().any(|&(first, last)| (first..=last).contains(&point))
}

fn is_word_character(character: char) -> bool {
  character.is_alphanumeric() || character == '_'
}

/// Whether `word` stands in `text` as a whole word, not as part of a longer one.
fn holds_word(text: &str, word: &str) -> bool {
  text.match_indices(word).any(|(start, found)| {
    let before = text[..start].chars().next_back();
    let after = text[start + found.len()..].chars().next();
    !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
  })
}

fn names_list(names: &toml::Table, from: &str, key: &str) -> Vec<String> {
  names
    .get(key)
    .and_then(toml::Value::as_array)
    .map(|listed| {
      listed
        .iter()
        .map(|name| name.as_str().unwrap_or_else(|| panic!("{from}: `{key}` lists strings")).to_owned())
        .collect()
    })
    .unwrap_or_default()
}

#[test]
fn depends_on_no_bmo_package_but_the_base_libraries() {
  let metadata = metadata();
  let mut refused = Vec::new();
  for package in metadata["packages"].as_array().expect("metadata lists packages") {
    let owner = package["name"].as_str().expect("a package has a name");
    for dependency in package["dependencies"].as_array().expect("a package lists its dependencies") {
      let name = dependency["name"].as_str().expect("a dependency has a name");
      let foreign_bmo = name.starts_with("bmo-") && !BASE_LIBRARIES.contains(&name);
      if foreign_bmo || OLD_SHARED_CRATES.contains(&name) {
        refused.push(format!("{owner} -> {name}"));
      }
    }
  }
  assert!(refused.is_empty(), "dependencies outside the base libraries: {refused:?}");
}

#[test]
fn no_package_takes_an_old_shared_crate_name() {
  let metadata = metadata();
  let taken: Vec<&str> = metadata["packages"]
    .as_array()
    .expect("metadata lists packages")
    .iter()
    .map(|package| package["name"].as_str().expect("a package has a name"))
    .filter(|name| OLD_SHARED_CRATES.contains(name))
    .collect();
  assert!(taken.is_empty(), "packages named after old shared crates: {taken:?}");
}

#[test]
fn the_version_file_holds_the_workspaces_version() {
  let version = text_of("VERSION").expect("VERSION is at the root");
  assert_eq!(version.trim(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_testbed_declaration_is_in_the_testbeds_format() {
  let declaration: toml::Table = text_of("testbed.toml")
    .expect("testbed.toml is at the root")
    .parse()
    .expect("testbed.toml is TOML");
  let tables = ["test", "projects", "tools", "check", "build", "deploy", "runner", "events"];
  let strange: Vec<&String> = declaration
    .keys()
    .filter(|key| !["testbed", "name"].contains(&key.as_str()) && !tables.contains(&key.as_str()))
    .collect();

  assert_eq!(declaration.get("testbed").and_then(toml::Value::as_integer), Some(0));
  assert!(strange.is_empty(), "testbed.toml holds keys its format does not name: {strange:?}");
  for key in tables.iter().filter(|key| declaration.contains_key(**key)) {
    assert!(declaration[*key].is_table(), "`{key}` in testbed.toml is a table");
  }
  assert_eq!(
    declaration["test"].get("languages").and_then(toml::Value::as_array),
    Some(&vec![toml::Value::from("cargo"), toml::Value::from("bun")])
  );
  assert_eq!(
    declaration["check"].get("base_libraries").and_then(toml::Value::as_array),
    Some(&BASE_LIBRARIES.iter().map(|&name| toml::Value::from(name)).collect())
  );
}

#[test]
fn only_cargo_manifests_at_the_root() {
  let found: Vec<String> = fs::read_dir(ROOT)
    .expect("the repository root is readable")
    .map(|entry| entry.expect("a root entry is readable").file_name().to_string_lossy().into_owned())
    .filter(|name| OTHER_MANIFESTS.contains(&name.as_str()))
    .collect();
  assert!(found.is_empty(), "the root holds {found:?}; only Cargo manifests belong there");
}

#[test]
fn another_build_tools_manifest_stands_only_in_the_reader() {
  let found: Vec<String> = tracked_files()
    .into_iter()
    .filter(|path| {
      let file = Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned());
      file.is_some_and(|name| OTHER_MANIFESTS.contains(&name.as_str()))
    })
    .filter(|path| Path::new(path).parent() != Some(Path::new(READER)))
    .collect();
  assert!(found.is_empty(), "manifests of another build tool outside `{READER}/`: {found:?}");
}

#[test]
fn the_reader_depends_on_no_bmo_package_and_no_other_checkout() {
  let Some(manifest) = text_of(&format!("{READER}/package.json")) else {
    return;
  };
  let manifest: Value = serde_json::from_str(&manifest).expect("the reader's package.json is JSON");
  let mut refused = Vec::new();
  for table in READER_DEPENDENCY_TABLES {
    let Some(dependencies) = manifest.get(table).and_then(Value::as_object) else {
      continue;
    };
    for (name, specifier) in dependencies {
      let specifier = specifier.as_str().unwrap_or_default();
      if name.starts_with("@bmo/") || LOCAL_SPECIFIERS.iter().any(|local| specifier.starts_with(local)) {
        refused.push(format!("{table}: {name} {specifier}"));
      }
    }
  }
  assert!(refused.is_empty(), "the reader reaches outside itself: {refused:?}");
}

#[test]
fn the_reader_carries_the_repositorys_version() {
  let Some(manifest) = text_of(&format!("{READER}/package.json")) else {
    return;
  };
  let manifest: Value = serde_json::from_str(&manifest).expect("the reader's package.json is JSON");
  assert_eq!(manifest["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
}

#[test]
fn no_directory_is_named_after_its_parent() {
  let twins: Vec<String> = tracked_files()
    .into_iter()
    .filter(|path| {
      let parts: Vec<Component> = Path::new(path).components().collect();
      parts.windows(2).take(parts.len().saturating_sub(2)).any(|pair| pair[0] == pair[1])
    })
    .collect();
  assert!(twins.is_empty(), "these paths repeat a directory inside itself: {twins:?}");
}

/// The fixed API, whose marker names the version it is fixed at.
const FIXED_API: &str = "docs/api.md";

#[test]
fn no_hangul_in_tracked_files() {
  let found: Vec<String> = tracked_texts()
    .into_iter()
    .filter(|(_, text)| text.chars().any(is_hangul))
    .map(|(path, _)| path)
    .collect();
  assert!(found.is_empty(), "tracked files holding Hangul: {found:?}");
}

#[test]
fn the_fixed_api_document_carries_the_fixed_marker_of_its_version() {
  let text = text_of(FIXED_API).expect("docs/api.md is tracked");
  let version = fs::read_to_string(Path::new(ROOT).join("VERSION")).expect("VERSION is readable");
  let marker = format!("**Fixed at {}.**", version.trim());
  let first = text.lines().skip(1).find(|line| !line.trim().is_empty());

  assert_eq!(first, Some(marker.as_str()), "the first line under the title is the fixed marker");
}

/// The names list, as `BMO_NAMES_LIST` names it: `words` matched as whole words ignoring case,
/// `exact_words` as whole words with their case kept, `anywhere` matched anywhere (a Korean name takes its
/// particles with no space between), and `allowed`, terms of another unit's protocol that carry a name as
/// part of themselves, set aside before the names are matched. Where no list is named, the names are not
/// checked here and the run says so.
#[test]
fn no_listed_names_in_tracked_files() {
  let Some(from) = std::env::var_os(NAMES_LIST) else {
    eprintln!("{NAMES_LIST} names no list; the names were not checked");
    return;
  };
  let from = from.to_string_lossy().into_owned();
  let names: toml::Table = fs::read_to_string(&from)
    .unwrap_or_else(|error| panic!("{from} is unreadable: {error}"))
    .parse()
    .unwrap_or_else(|error| panic!("{from} is not TOML: {error}"));
  let words: Vec<String> =
    names_list(&names, &from, "words").iter().map(|name| name.to_lowercase()).collect();
  let exact_words = names_list(&names, &from, "exact_words");
  let anywhere = names_list(&names, &from, "anywhere");
  let allowed = names_list(&names, &from, "allowed");
  let mut found = Vec::new();
  for (path, text) in tracked_texts() {
    let text = allowed.iter().fold(text, |text, term| text.replace(term.as_str(), " "));
    let lowered = text.to_lowercase();
    let held = words
      .iter()
      .filter(|name| holds_word(&lowered, name))
      .chain(exact_words.iter().filter(|name| holds_word(&text, name)))
      .chain(anywhere.iter().filter(|name| text.contains(name.as_str())));
    found.extend(held.map(|name| format!("{path}: {name}")));
  }
  assert!(found.is_empty(), "listed names in tracked files: {found:?}");
}
