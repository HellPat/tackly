//! The SQLite database: opening it, and reading columns without panicking.

use std::{str::FromStr, time::Duration};

use sqlx::{
    Decode, Row, Sqlite, SqlitePool, Type,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
};
use uuid::Uuid;

use crate::error::{ApiError, internal};

/// Opens (creating if needed) the database with a single writer connection,
/// which also serializes the allocation of event sequence numbers.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10))
        .journal_mode(SqliteJournalMode::Wal);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

/// Applies the migrations that are built into the binary.
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// How far past the last sequence number a start moves the counter: more
/// messages than a backup can miss.
pub const RESTART_GAP: i64 = 1_000_000;

/// Records this start of the relay (see migrations/0009_restarts.sql): what
/// was accepted after the last sequence number in the database may be lost,
/// and numbering goes on past a gap. Nothing to record before the first
/// message.
pub async fn record_restart(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let last: Option<i64> =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name='group_messages'")
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(after) = last.filter(|last| *last > 0) {
        let sequence = after + RESTART_GAP;
        sqlx::query("INSERT INTO restarts (sequence, after) VALUES (?,?)")
            .bind(sequence)
            .bind(after)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE sqlite_sequence SET seq=? WHERE name='group_messages'")
            .bind(sequence)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// A column of a row, or a 500 if the query and the code disagree.
pub fn column<'r, T>(row: &'r SqliteRow, name: &str) -> Result<T, ApiError>
where
    T: Decode<'r, Sqlite> + Type<Sqlite>,
{
    row.try_get(name).map_err(internal)
}

/// A column holding a UUID as text.
pub fn uuid_column(row: &SqliteRow, name: &str) -> Result<Uuid, ApiError> {
    let text: String = column(row, name)?;
    Uuid::parse_str(&text).map_err(|error| {
        eprintln!("stored value in {name} is not a UUID: {error}");
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "database error",
        )
    })
}
