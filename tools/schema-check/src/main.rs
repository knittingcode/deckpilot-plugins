//! `schema-check --schemas DIR SCHEMA_REF FILE...`
//!
//! Validates each FILE against SCHEMA_REF, a schema `$id` with an optional
//! JSON pointer fragment (for example
//! `https://knittingcode.github.io/deckpilot-plugins/v1/schemas/registry-plugin.schema.json#/$defs/version`).
//! Every `*.schema.json` below DIR is loaded and served by its `$id`; no
//! reference is ever fetched from the network or another path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};

struct Local(HashMap<String, Value>);

impl jsonschema::Retrieve for Local {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        self.0
            .get(uri.as_str())
            .cloned()
            .ok_or_else(|| format!("unknown schema reference {uri}").into())
    }
}

fn load_schemas(dir: &Path, out: &mut HashMap<String, Value>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            load_schemas(&path, out)?;
        } else if path.to_string_lossy().ends_with(".schema.json") {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let schema: Value =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let id = schema["$id"]
                .as_str()
                .ok_or_else(|| format!("{} has no $id", path.display()))?
                .to_owned();
            if out.insert(id.clone(), schema).is_some() {
                return Err(format!("two schemas have the $id {id}"));
            }
        }
    }
    Ok(())
}

fn run() -> Result<bool, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [flag, dir, schema_ref, files @ ..] = args.as_slice() else {
        return Err("usage: schema-check --schemas DIR SCHEMA_REF FILE...".into());
    };
    if flag != "--schemas" || files.is_empty() {
        return Err("usage: schema-check --schemas DIR SCHEMA_REF FILE...".into());
    }
    let mut schemas = HashMap::new();
    load_schemas(Path::new(dir), &mut schemas)?;
    let base = schema_ref.split('#').next().unwrap_or_default();
    if !schemas.contains_key(base) {
        return Err(format!("no schema with the $id {base} in {dir}"));
    }
    let root = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": schema_ref,
    });
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .with_retriever(Local(schemas))
        .build(&root)
        .map_err(|e| format!("{schema_ref}: {e}"))?;
    let mut ok = true;
    for file in files {
        let instance: Value = match std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(value) => value,
            Err(e) => {
                eprintln!("{file}: {e}");
                ok = false;
                continue;
            }
        };
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|e| format!("{file}: {}: {e}", e.instance_path()))
            .collect();
        if errors.is_empty() {
            println!("{file}: valid");
        } else {
            ok = false;
            for error in errors {
                eprintln!("{error}");
            }
        }
    }
    Ok(ok)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("schema-check: {e}");
            ExitCode::from(2)
        }
    }
}
