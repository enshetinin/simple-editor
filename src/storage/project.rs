//! Project files under `<project>/.simpleeditor/` (readable JSON, meant for Git):
//!
//! - `collections/<id>.json`  — `{ "schema_version": 1, "name", "requests": [...] }`
//! - `environments/<id>.json` — `{ "schema_version": 1, "name", "variables": [...] }`
//! - `variables.json`         — `{ "schema_version": 1, "variables": [...] }`
//!
//! Nothing is created until the user saves a collection or environment.
//! Secret values are never written here. Files with an unknown schema version
//! or invalid JSON are reported and left untouched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::write_json;
use crate::http::model::{RequestDefinition, Variable};
use crate::http::sanitize::strip_secrets;

pub const DIR: &str = ".simpleeditor";
pub const SCHEMA_VERSION: u32 = 1;
/// Largest collection/environment file read or imported.
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionFile {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub requests: Vec<RequestDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentFile {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariablesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub variables: Vec<Variable>,
}

/// A collection loaded from (or to be saved to) the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// File stem, unique within the project.
    pub id: String,
    pub name: String,
    pub requests: Vec<RequestDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectData {
    pub collections: Vec<Collection>,
    pub environments: Vec<Environment>,
    pub variables: Vec<Variable>,
    /// Files that could not be loaded, with the reason.
    pub errors: Vec<String>,
}

pub fn collections_dir(root: &Path) -> PathBuf {
    root.join(DIR).join("collections")
}

pub fn environments_dir(root: &Path) -> PathBuf {
    root.join(DIR).join("environments")
}

pub fn variables_path(root: &Path) -> PathBuf {
    root.join(DIR).join("variables.json")
}

fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!(
            "the file is larger than {} MiB",
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }
    fs::read(path).map_err(|e| e.to_string())
}

/// Parses JSON that carries a `schema_version`, checking it first so a file
/// from a newer version gets a clear message instead of a field error.
fn parse_versioned<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, String> {
    #[derive(Deserialize)]
    struct Version {
        schema_version: Option<u32>,
    }
    let v: Version = serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    match v.schema_version {
        Some(SCHEMA_VERSION) => {}
        Some(other) => {
            return Err(format!(
                "schema_version {other} is not supported (this SimpleEditor reads version {SCHEMA_VERSION}); the file was not changed"
            ));
        }
        None => return Err("missing schema_version; not a SimpleEditor file".to_owned()),
    }
    serde_json::from_slice(bytes).map_err(|e| format!("invalid content: {e}"))
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file())
        .collect();
    files.sort();
    files
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Loads everything under `.simpleeditor/`. Never writes.
pub fn load(root: &Path) -> ProjectData {
    let mut data = ProjectData::default();
    for path in json_files(&collections_dir(root)) {
        match read_limited(&path).and_then(|b| parse_versioned::<CollectionFile>(&b)) {
            Ok(f) => data.collections.push(Collection {
                id: stem(&path),
                name: f.name,
                requests: with_ids(f.requests),
            }),
            Err(e) => data.errors.push(format!("{}: {e}", path.display())),
        }
    }
    for path in json_files(&environments_dir(root)) {
        match read_limited(&path).and_then(|b| parse_versioned::<EnvironmentFile>(&b)) {
            Ok(f) => data.environments.push(Environment {
                id: stem(&path),
                name: f.name,
                variables: f.variables,
            }),
            Err(e) => data.errors.push(format!("{}: {e}", path.display())),
        }
    }
    let vars = variables_path(root);
    if vars.exists() {
        match read_limited(&vars).and_then(|b| parse_versioned::<VariablesFile>(&b)) {
            Ok(f) => data.variables = f.variables,
            Err(e) => data.errors.push(format!("{}: {e}", vars.display())),
        }
    }
    data
}

/// Gives every request a unique id (files edited by hand may lack them).
fn with_ids(mut requests: Vec<RequestDefinition>) -> Vec<RequestDefinition> {
    let mut seen = std::collections::HashSet::new();
    for r in &mut requests {
        if r.id.is_empty() || !seen.insert(r.id.clone()) {
            r.id = crate::http::model::new_id();
            seen.insert(r.id.clone());
        }
    }
    requests
}

/// A file-name-safe id derived from a display name, unique in `dir`.
pub fn unique_id(dir: &Path, name: &str) -> String {
    let mut base: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if base.is_empty() {
        base = "untitled".to_owned();
    }
    base.truncate(60);
    let mut id = base.clone();
    let mut n = 2;
    while dir.join(format!("{id}.json")).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn clean_variables(vars: &[Variable]) -> Vec<Variable> {
    vars.iter()
        .map(|v| {
            let mut v = v.clone();
            if v.secret {
                v.value.clear();
            }
            v
        })
        .collect()
}

/// Saves a collection without literal credentials. Returns the fields whose
/// secret values were left out.
pub fn save_collection(root: &Path, collection: &Collection) -> io::Result<Vec<String>> {
    let mut omitted = Vec::new();
    let requests = collection
        .requests
        .iter()
        .map(|r| {
            let (clean, fields) = strip_secrets(r);
            omitted.extend(fields.into_iter().map(|f| format!("{} › {f}", r.name)));
            clean
        })
        .collect();
    let file = CollectionFile {
        schema_version: SCHEMA_VERSION,
        name: collection.name.clone(),
        requests,
    };
    write_json(
        &collections_dir(root).join(format!("{}.json", collection.id)),
        &file,
    )?;
    Ok(omitted)
}

pub fn delete_collection(root: &Path, id: &str) -> io::Result<()> {
    fs::remove_file(collections_dir(root).join(format!("{id}.json")))
}

pub fn save_environment(root: &Path, env: &Environment) -> io::Result<()> {
    let file = EnvironmentFile {
        schema_version: SCHEMA_VERSION,
        name: env.name.clone(),
        variables: clean_variables(&env.variables),
    };
    write_json(
        &environments_dir(root).join(format!("{}.json", env.id)),
        &file,
    )
}

pub fn delete_environment(root: &Path, id: &str) -> io::Result<()> {
    fs::remove_file(environments_dir(root).join(format!("{id}.json")))
}

pub fn save_variables(root: &Path, variables: &[Variable]) -> io::Result<()> {
    let file = VariablesFile {
        schema_version: SCHEMA_VERSION,
        variables: clean_variables(variables),
    };
    write_json(&variables_path(root), &file)
}

/// Exports a collection to any path. Secrets are left out unless asked.
pub fn export_collection(
    collection: &Collection,
    path: &Path,
    include_secrets: bool,
) -> io::Result<()> {
    let requests = collection
        .requests
        .iter()
        .map(|r| {
            if include_secrets {
                r.clone()
            } else {
                strip_secrets(r).0
            }
        })
        .collect();
    write_json(
        path,
        &CollectionFile {
            schema_version: SCHEMA_VERSION,
            name: collection.name.clone(),
            requests,
        },
    )
}

/// Reads a collection export and adds it to the project under a new id;
/// an existing collection is never overwritten (the name gets a suffix).
pub fn import_collection(
    root: &Path,
    source: &Path,
    existing_names: &[String],
) -> Result<Collection, String> {
    let bytes = read_limited(source)?;
    let file: CollectionFile = parse_versioned(&bytes)?;
    let mut name = file.name.trim().to_owned();
    if name.is_empty() {
        name = "Imported".to_owned();
    }
    if existing_names.iter().any(|n| n == &name) {
        let mut n = 2;
        while existing_names.iter().any(|e| *e == format!("{name} ({n})")) {
            n += 1;
        }
        name = format!("{name} ({n})");
    }
    let collection = Collection {
        id: unique_id(&collections_dir(root), &name),
        name,
        requests: with_ids(file.requests),
    };
    save_collection(root, &collection).map_err(|e| e.to_string())?;
    Ok(collection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::model::{Auth, KeyValue};

    fn req(name: &str) -> RequestDefinition {
        RequestDefinition {
            name: name.into(),
            url: "{{base}}/users".into(),
            params: vec![KeyValue::new("page", "1")],
            auth: Auth::Bearer {
                token: "literal-token".into(),
            },
            ..RequestDefinition::default()
        }
    }

    #[test]
    fn nothing_is_created_until_saved_and_round_trip_strips_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let data = load(root);
        assert!(data.collections.is_empty() && data.errors.is_empty());
        assert!(!root.join(DIR).exists(), "loading must not create files");

        let c = Collection {
            id: unique_id(&collections_dir(root), "Users API"),
            name: "Users API".into(),
            requests: vec![req("List")],
        };
        assert_eq!(c.id, "users-api");
        let omitted = save_collection(root, &c).unwrap();
        assert_eq!(omitted, vec!["List › Auth › Token".to_owned()]);
        let text = fs::read_to_string(collections_dir(root).join("users-api.json")).unwrap();
        assert!(text.contains("\"schema_version\": 1"));
        assert!(!text.contains("literal-token"));
        assert!(text.ends_with("}\n"));

        let env = Environment {
            id: "local".into(),
            name: "Local".into(),
            variables: vec![
                Variable::plain("base", "http://localhost:8080"),
                Variable::secret("token", "s3cr3t"),
            ],
        };
        save_environment(root, &env).unwrap();
        save_variables(root, &[Variable::plain("version", "v1")]).unwrap();
        let env_text = fs::read_to_string(environments_dir(root).join("local.json")).unwrap();
        assert!(!env_text.contains("s3cr3t"));
        assert!(env_text.contains("\"secret\": true"));

        let data = load(root);
        assert_eq!(data.collections.len(), 1);
        assert_eq!(data.collections[0].requests[0].params[0].value, "1");
        assert_eq!(
            data.environments[0].variables[1],
            Variable::secret("token", "")
        );
        assert_eq!(data.variables, vec![Variable::plain("version", "v1")]);
    }

    #[test]
    fn incompatible_or_invalid_files_are_reported_and_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cdir = collections_dir(root);
        fs::create_dir_all(&cdir).unwrap();
        let future = r#"{"schema_version": 2, "name": "From the future", "requests": []}"#;
        fs::write(cdir.join("future.json"), future).unwrap();
        fs::write(cdir.join("broken.json"), "{ nope").unwrap();
        let data = load(root);
        assert!(data.collections.is_empty());
        assert_eq!(data.errors.len(), 2);
        assert!(data.errors.iter().any(|e| e.contains("schema_version 2")));
        // A new collection with the same name never clobbers the unknown file.
        let id = unique_id(&cdir, "future");
        assert_eq!(id, "future-2");
        assert_eq!(
            fs::read_to_string(cdir.join("future.json")).unwrap(),
            future
        );
    }

    #[test]
    fn export_import_keeps_structure_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = Collection {
            id: "api".into(),
            name: "API".into(),
            requests: vec![req("A"), req("B")],
        };
        save_collection(root, &c).unwrap();
        let out = dir.path().join("export.json");
        export_collection(&c, &out, false).unwrap();
        assert!(!fs::read_to_string(&out).unwrap().contains("literal-token"));

        let imported = import_collection(root, &out, &["API".to_owned()]).unwrap();
        assert_eq!(imported.name, "API (2)");
        assert_ne!(imported.id, "api");
        assert_eq!(imported.requests.len(), 2);
        assert_eq!(imported.requests[1].name, "B");
        let data = load(root);
        assert_eq!(data.collections.len(), 2);

        export_collection(&c, &out, true).unwrap();
        assert!(fs::read_to_string(&out).unwrap().contains("literal-token"));

        fs::write(&out, r#"{"schema_version": 9, "name": "x"}"#).unwrap();
        let err = import_collection(root, &out, &[]).unwrap_err();
        assert!(err.contains("schema_version 9"), "{err}");
        assert_eq!(load(root).collections.len(), 2, "nothing added on error");
    }
}
