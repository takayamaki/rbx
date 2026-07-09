mod common;

use rbx::helpers::{allocate_usns, generate_numeric_id, now_datetime};

/// rekordbox silently ignores rows whose timestamps are not in the exact
/// native format (milliseconds + timezone offset). This regression was found
/// the hard way: tracks written with second-precision timestamps never showed
/// up in rekordbox.
#[test]
fn now_datetime_matches_rekordbox_native_format() {
    let ts = now_datetime();
    let re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} \+00:00$").unwrap();
    assert!(re.is_match(&ts), "unexpected timestamp format: {}", ts);
}

/// masterPlaylists6.xml references playlists by the hex form of their numeric
/// ID, so generated IDs must be numeric 28-bit values like rekordbox's own.
#[tokio::test]
async fn generate_numeric_id_produces_28bit_numeric_strings() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;

    for _ in 0..50 {
        let id = generate_numeric_id(&pool, "djmdPlaylist").await.unwrap();
        assert!(id.chars().all(|c| c.is_ascii_digit()), "non-numeric ID: {}", id);
        let n: u64 = id.parse().unwrap();
        assert!(n >= 100, "ID below minimum: {}", n);
        assert!(n < (1 << 28), "ID exceeds 28 bits: {}", n);
    }
}

#[tokio::test]
async fn generate_numeric_id_avoids_existing_ids() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;

    // The seeded playlist occupies ID '501'; new IDs must never collide
    // with any existing row.
    for _ in 0..20 {
        let id = generate_numeric_id(&pool, "djmdPlaylist").await.unwrap();
        assert_ne!(id, "501");
        let exists = sqlx::query("SELECT 1 FROM djmdPlaylist WHERE ID = ?")
            .bind(&id)
            .fetch_optional(&pool)
            .await
            .unwrap();
        assert!(exists.is_none(), "generated ID already in table: {}", id);
    }
}

/// USNs must come from the global counter in agentRegistry. Rows whose
/// rb_local_usn exceeds localUpdateCount are ignored by rekordbox sync —
/// this was the root cause of playlist tracks not appearing.
#[tokio::test]
async fn allocate_usns_advances_agent_registry_counter() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;

    async fn counter(pool: &sqlx::SqlitePool) -> i64 {
        let (v,): (i64,) = sqlx::query_as(
            "SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        v
    }

    assert_eq!(counter(&pool).await, 1000);

    let first = allocate_usns(&pool, 1).await.unwrap();
    assert_eq!(first, 1001);
    assert_eq!(counter(&pool).await, 1001);

    let first = allocate_usns(&pool, 5).await.unwrap();
    assert_eq!(first, 1002);
    let after = counter(&pool).await;
    assert_eq!(after, 1006);
    // The last USN of the allocated block must equal the counter,
    // so no row can ever exceed it.
    assert_eq!(first + 5 - 1, after);
}
