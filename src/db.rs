//! SQLite connection pool and embedded migrations.

use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

/// Opens a pool and applies the migrations compiled into the binary.
///
/// An in-memory database exists per connection, so it is limited to one connection that is
/// never recycled; otherwise the schema would vanish.
pub async fn connect(url: &str, max_connections: u32) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    let in_memory = url.contains(":memory:");
    let pool = SqlitePoolOptions::new()
        .max_connections(if in_memory { 1 } else { max_connections })
        .min_connections(u32::from(in_memory))
        .idle_timeout(if in_memory {
            None
        } else {
            Some(Duration::from_secs(600))
        })
        .max_lifetime(None)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
