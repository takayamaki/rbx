use serde_json::Value;

pub const SCHEMA_VERSION: &str = "v1";

pub fn success(kind: &str, items: Value) -> Value {
    serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "items": items,
    })
}

pub fn success_one(kind: &str, item: Value) -> Value {
    serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "item": item,
    })
}

pub fn mutation_done(kind: &str, detail: Value) -> Value {
    serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "dry_run": false,
        "result": detail,
    })
}

pub fn mutation_dry_run(kind: &str, plan: Value, execute_hint: &str) -> Value {
    serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "dry_run": true,
        "plan": plan,
        "next_step": execute_hint,
    })
}

pub fn error(category: &str, exit_code: i32, message: &str, next_step: Option<&str>) -> Value {
    let mut err = serde_json::json!({
        "category": category,
        "exit_code": exit_code,
        "message": message,
    });
    if let Some(ns) = next_step {
        err.as_object_mut().unwrap().insert("next_step".to_string(), Value::String(ns.to_string()));
    }
    serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "kind": "error",
        "error": err,
    })
}

pub fn print(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

// Semantic exit codes
pub const EXIT_OK: i32 = 0;
pub const EXIT_GENERAL: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_NOT_FOUND: i32 = 3;
pub const EXIT_CONFIG: i32 = 4;
pub const EXIT_CONFLICT: i32 = 5;
