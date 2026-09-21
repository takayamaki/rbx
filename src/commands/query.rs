use rbx::output;
use sqlx::sqlite::SqlitePool;
use sqlx::{Column, Row as _};

use crate::commands::db_error;

/// Without --unsafe-write only investigation statements are allowed:
/// SELECT, WITH (CTEs), PRAGMA (schema inspection), EXPLAIN.
fn is_read_only_statement(sql: &str) -> bool {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.contains(';') {
        // Reject multi-statement input outright; a write could hide behind
        // a leading SELECT.
        return false;
    }
    let first = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(first.as_str(), "SELECT" | "WITH" | "PRAGMA" | "EXPLAIN")
}

pub(crate) async fn handle_query(
    pool: &SqlitePool,
    sql: &str,
    unsafe_write: bool,
) -> (serde_json::Value, i32) {
    if !unsafe_write && !is_read_only_statement(sql) {
        return (
            output::error(
                "readonly",
                output::EXIT_USAGE,
                "query allows only SELECT / WITH / PRAGMA / EXPLAIN (single statement) by default",
                Some(
                    "Prefer a dedicated command (they maintain rekordbox invariants: \
                      USN allocation, timestamp format, numeric IDs, masterPlaylists6.xml sync). \
                      If you really need raw SQL writes, re-run with --unsafe-write",
                ),
            ),
            output::EXIT_USAGE,
        );
    }
    match sqlx::query(sql).fetch_all(pool).await {
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    let cols = row.columns();
                    let mut obj = serde_json::Map::new();
                    for col in cols {
                        let name = col.name().to_string();
                        let val = row
                            .try_get::<String, _>(col.ordinal())
                            .map(serde_json::Value::String)
                            .or_else(|_| {
                                row.try_get::<i64, _>(col.ordinal())
                                    .map(|v| serde_json::json!(v))
                            })
                            .or_else(|_| {
                                row.try_get::<f64, _>(col.ordinal())
                                    .map(|v| serde_json::json!(v))
                            })
                            .unwrap_or(serde_json::Value::Null);
                        obj.insert(name, val);
                    }
                    serde_json::Value::Object(obj)
                })
                .collect();
            (
                output::success("query_result", serde_json::Value::Array(items)),
                output::EXIT_OK,
            )
        }
        Err(e) if e.to_string().contains("readonly database") => (
            output::error(
                "readonly",
                output::EXIT_USAGE,
                "query runs read-only by default; this statement needs write access",
                Some(
                    "Prefer a dedicated command (they maintain rekordbox invariants). \
                      If you really need raw SQL, re-run with --unsafe-write",
                ),
            ),
            output::EXIT_USAGE,
        ),
        Err(e) => db_error(e),
    }
}
