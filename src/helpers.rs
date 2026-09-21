use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

/// Current time in rekordbox's native timestamp format.
/// rekordbox silently ignores rows whose timestamps lack the millisecond
/// and timezone suffix.
pub fn now_datetime() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S%.3f +00:00")
        .to_string()
}

/// Allocates `count` sequential USNs from the global counter in agentRegistry
/// (registry_id = 'localUpdateCount'), advancing it. Returns the first
/// allocated USN. rekordbox expects row rb_local_usn values to never exceed
/// this counter.
pub async fn allocate_usns(pool: &SqlitePool, count: i64) -> Result<i64, sqlx::Error> {
    let (current,): (i64,) =
        sqlx::query_as("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'")
            .fetch_one(pool)
            .await?;
    sqlx::query("UPDATE agentRegistry SET int_1 = ? WHERE registry_id = 'localUpdateCount'")
        .bind(current + count)
        .execute(pool)
        .await?;
    Ok(current + 1)
}

/// Generates an unused numeric 28-bit ID like rekordbox natively uses.
/// masterPlaylists6.xml stores playlist IDs in hex, so playlist IDs must be
/// numeric — UUID-style IDs are invisible to rekordbox.
pub async fn generate_numeric_id(pool: &SqlitePool, table: &str) -> Result<String, sqlx::Error> {
    loop {
        let bytes = *Uuid::new_v4().as_bytes();
        let raw = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let id = raw >> 4; // 28-bit
        if id < 100 {
            continue;
        }
        let id_str = id.to_string();
        let sql = format!("SELECT 1 FROM {} WHERE ID = ?", table);
        let exists = sqlx::query(&sql).bind(&id_str).fetch_optional(pool).await?;
        if exists.is_none() {
            return Ok(id_str);
        }
    }
}
