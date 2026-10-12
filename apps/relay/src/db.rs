//! D1, the relay's store: prepared statements with bound values, rows read
//! into structs, and batches, which D1 runs as one transaction. D1 has no
//! transactions across calls; the Durable Object lets one request run at a
//! time, so a handler reads, decides, and writes one batch.

use js_sys::Uint8Array;
use serde::de::DeserializeOwned;
use uuid::Uuid;
use wasm_bindgen::JsValue;
use worker::{D1Database, D1PreparedStatement};

use crate::error::{ApiError, internal};

/// A value bound to a `?` in a statement.
pub enum Arg {
    Text(String),
    Int(i64),
    Blob(Vec<u8>),
    Null,
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Arg {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Uuid> for Arg {
    fn from(value: Uuid) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<i64> for Arg {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<&[u8]> for Arg {
    fn from(value: &[u8]) -> Self {
        Self::Blob(value.to_vec())
    }
}

impl From<Vec<u8>> for Arg {
    fn from(value: Vec<u8>) -> Self {
        Self::Blob(value)
    }
}

impl From<Option<String>> for Arg {
    fn from(value: Option<String>) -> Self {
        value.map_or(Self::Null, Self::Text)
    }
}

impl Arg {
    fn to_js(&self) -> JsValue {
        match self {
            Self::Text(text) => JsValue::from_str(text),
            // D1 has no BigInt: integers travel as numbers (53 bits).
            Self::Int(number) => JsValue::from_f64(*number as f64),
            // D1 stores an ArrayBuffer as a BLOB.
            Self::Blob(bytes) => Uint8Array::from(bytes.as_slice()).buffer().into(),
            Self::Null => JsValue::null(),
        }
    }
}

/// `args![a, b]`: the values for a statement's `?`s, in order.
macro_rules! args {
    ($($value:expr),* $(,)?) => {
        vec![$($crate::db::Arg::from($value)),*]
    };
}
pub(crate) use args;

pub struct Db(pub D1Database);

impl Db {
    pub fn statement(&self, sql: &str, args: Vec<Arg>) -> Result<D1PreparedStatement, ApiError> {
        let values: Vec<JsValue> = args.iter().map(Arg::to_js).collect();
        self.0.prepare(sql).bind(&values).map_err(internal)
    }

    /// Every row, read into `T` (BLOBs come back as byte lists, integers as
    /// numbers, NULL as `None`).
    pub async fn all<T: DeserializeOwned>(
        &self,
        sql: &str,
        args: Vec<Arg>,
    ) -> Result<Vec<T>, ApiError> {
        self.statement(sql, args)?
            .all()
            .await
            .map_err(internal)?
            .results()
            .map_err(internal)
    }

    pub async fn first<T: DeserializeOwned>(
        &self,
        sql: &str,
        args: Vec<Arg>,
    ) -> Result<Option<T>, ApiError> {
        Ok(self.all(sql, args).await?.into_iter().next())
    }

    /// Runs one statement; how many rows it changed.
    pub async fn run(&self, sql: &str, args: Vec<Arg>) -> Result<usize, ApiError> {
        let result = self.statement(sql, args)?.run().await.map_err(internal)?;
        changes(&result)
    }

    /// Runs the statements as one transaction: all of them or none.
    pub async fn batch(&self, statements: Vec<D1PreparedStatement>) -> Result<(), ApiError> {
        if statements.is_empty() {
            return Ok(());
        }
        self.0.batch(statements).await.map_err(internal)?;
        Ok(())
    }
}

fn changes(result: &worker::D1Result) -> Result<usize, ApiError> {
    Ok(result
        .meta()
        .map_err(internal)?
        .and_then(|meta| meta.changes)
        .unwrap_or(0))
}
