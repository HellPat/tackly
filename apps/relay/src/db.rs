//! The SQLite database: opening it, and reading columns without panicking.

use std::{str::FromStr, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sqlx::{
    Decode, Row, Sqlite, SqlitePool, Type,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
};
use uuid::Uuid;

use crate::error::{ApiError, bad_request, internal};

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

/// Binary data as base64url, the way it travels in JSON.
pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Decodes a base64url request field, optionally demanding an exact length.
pub fn decode(input: &str, expected_len: Option<usize>) -> Result<Vec<u8>, ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(input)
        .map_err(|_| bad_request("invalid base64url value"))?;
    if expected_len.is_some_and(|len| bytes.len() != len) {
        return Err(bad_request("invalid value length"));
    }
    Ok(bytes)
}
