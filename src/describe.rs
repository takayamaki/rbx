use rbx::output;

use crate::commands::{history, mytags, playlists, query, tracks};

/// `rbx describe [resource] [action]`. Each command module describes its own
/// resource; this only routes and builds the shared envelopes.
pub(crate) fn handle_describe(
    resource: Option<String>,
    action: Option<String>,
) -> serde_json::Value {
    let action = action.as_deref();
    let described = match resource.as_deref() {
        None => return describe_root(),
        Some("tracks") => tracks::describe(action),
        Some("playlists") => playlists::describe(action),
        Some("mytags") => mytags::describe(action),
        Some("history") => history::describe(action),
        Some("query") => query::describe(action),
        Some(r) => {
            return output::error(
                "not_found",
                output::EXIT_NOT_FOUND,
                &format!("Unknown resource: {}", r),
                Some("Use 'rbx describe' to see available resources"),
            )
        }
    };
    described.unwrap_or_else(|| {
        let r = resource.as_deref().unwrap_or_default();
        output::error(
            "not_found",
            output::EXIT_NOT_FOUND,
            &format!("Unknown action: {} {}", r, action.unwrap_or_default()),
            Some(&format!(
                "Use 'rbx describe {}' to see available actions",
                r
            )),
        )
    })
}

fn describe_root() -> serde_json::Value {
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "resources": [
            { "name": "tracks", "description": "Query tracks and manage their My Tag assignments" },
            { "name": "playlists", "description": "Query playlists and their contents" },
            { "name": "mytags", "description": "Manage My Tag categories and tags (CRUD)" },
            { "name": "history", "description": "Query play history sessions" },
            { "name": "query", "description": "Run raw SQL against master.db (read-only: SELECT/WITH/PRAGMA/EXPLAIN)" },
        ],
        "global_flags": [
            { "name": "--db", "type": "path", "required": true, "env": "RBX_DB_PATH",
              "description": "Path to rekordbox master.db" },
        ],
        "discovery_sequence": [
            "rbx describe",
            "rbx describe <resource>",
            "rbx describe <resource> <action>",
            "rbx <resource> <action> [args]",
        ],
    })
}

pub(crate) fn describe_resource(name: &str, actions: &[(&str, &str)]) -> serde_json::Value {
    let acts: Vec<_> = actions
        .iter()
        .map(|(n, d)| {
            serde_json::json!({
                "name": n, "description": d,
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "resource": name,
        "actions": acts,
    })
}

pub(crate) fn describe_command(
    command: &str,
    flags: &[serde_json::Value],
    output_schema: &serde_json::Value,
    examples: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "command": command,
        "flags": flags,
        "output_schema": output_schema,
        "examples": examples,
    })
}

pub(crate) fn flag(name: &str, typ: &str, required: bool, desc: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "type": typ, "required": required, "description": desc })
}

pub(crate) fn mutation_result_schema(kind: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": format!("When dry_run=true, includes 'plan' and 'next_step'. When dry_run=false, includes 'result'. kind='{}'", kind),
        "properties": {
            "dry_run": { "type": "boolean" },
            "plan": { "type": "object", "description": "Present when dry_run=true" },
            "result": { "type": "object", "description": "Present when dry_run=false" },
            "next_step": { "type": "string", "description": "Present when dry_run=true" },
        },
    })
}
