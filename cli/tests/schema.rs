//! Validates every fixture against the JSON Schemas in `cli/schema/` (design 3 and 7).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn validator(file: &str) -> jsonschema::Validator {
    let schema = read_json(&crate_dir().join("schema").join(file));
    jsonschema::validator_for(&schema).unwrap_or_else(|e| panic!("compile {file}: {e}"))
}

/// The schema for a fixture follows its `type`, as the store will pick it.
fn schema_for<'a>(
    instance: &Value,
    annotation: &'a jsonschema::Validator,
    state: &'a jsonschema::Validator,
) -> Option<&'a jsonschema::Validator> {
    match instance.get("type").and_then(Value::as_str) {
        Some("Annotation") => Some(annotation),
        Some("marq:StateChange") => Some(state),
        _ => None,
    }
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("list {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
}

fn valid_fixtures() -> Vec<PathBuf> {
    json_files(&crate_dir().join("tests/fixtures"))
}

fn invalid_fixtures() -> Vec<PathBuf> {
    json_files(&crate_dir().join("tests/fixtures/invalid"))
}

#[test]
fn both_schemas_compile() {
    validator("annotation.schema.json");
    validator("state-change.schema.json");
}

#[test]
fn every_valid_fixture_passes() {
    let (annotation, state) = (
        validator("annotation.schema.json"),
        validator("state-change.schema.json"),
    );
    let files = valid_fixtures();
    assert!(
        files.len() >= 9,
        "expected at least 9 valid fixtures, found {}",
        files.len()
    );
    let mut failures = Vec::new();
    for path in &files {
        let instance = read_json(path);
        let Some(v) = schema_for(&instance, &annotation, &state) else {
            failures.push(format!("{}: no schema for its type", path.display()));
            continue;
        };
        for error in v.iter_errors(&instance) {
            failures.push(format!(
                "{}: {error} at {}",
                path.display(),
                error.instance_path()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "valid fixtures rejected:\n{}",
        failures.join("\n")
    );
}

#[test]
fn every_invalid_fixture_fails() {
    let (annotation, state) = (
        validator("annotation.schema.json"),
        validator("state-change.schema.json"),
    );
    let files = invalid_fixtures();
    assert!(
        files.len() >= 10,
        "expected at least 10 invalid fixtures, found {}",
        files.len()
    );
    let mut accepted = Vec::new();
    for path in &files {
        let instance = read_json(path);
        // A type that selects no schema cannot be stored, so it counts as rejected.
        let reasons: Vec<String> = match schema_for(&instance, &annotation, &state) {
            Some(v) => v.iter_errors(&instance).map(|e| e.to_string()).collect(),
            None => vec!["no schema for its type".to_string()],
        };
        if reasons.is_empty() {
            accepted.push(path.display().to_string());
        } else {
            println!("{}: rejected, as expected: {}", path.display(), reasons[0]);
        }
    }
    assert!(
        accepted.is_empty(),
        "invalid fixtures that the schema accepted (the schema is too loose, or the fixture is not invalid):\n{}",
        accepted.join("\n")
    );
}

#[test]
fn fixtures_are_in_canonical_form() {
    // Design 3.1: sorted keys, two-space indentation, final newline.
    // serde_json::Value sorts keys because the crate does not enable `preserve_order`.
    let mut wrong = Vec::new();
    for path in valid_fixtures().into_iter().chain(invalid_fixtures()) {
        let text = fs::read_to_string(&path).expect("read fixture");
        let value: Value = serde_json::from_str(&text).expect("parse fixture");
        let canonical = serde_json::to_string_pretty(&value).expect("serialise") + "\n";
        if text != canonical {
            wrong.push(path.display().to_string());
        }
    }
    assert!(
        wrong.is_empty(),
        "fixtures not in canonical form:\n{}",
        wrong.join("\n")
    );
}
