//! The pieces the step definitions are built from.
//!
//! Everything here returns `Result<_, Failure>`. Cucumber ends a scenario by
//! a panic, so the step functions pass each outcome through [`check`], the one
//! place where a failure becomes a panic.

pub mod member;
pub mod page;

/// Why a step did not work, in words for the person reading the test output.
#[derive(Debug)]
pub struct Failure(pub String);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

pub type Outcome<T> = Result<T, Failure>;

macro_rules! failure_from {
    ($($error:ty),* $(,)?) => {$(
        impl From<$error> for Failure {
            fn from(error: $error) -> Self {
                Self(error.to_string())
            }
        }
    )*};
}
failure_from!(
    std::io::Error,
    serde_json::Error,
    base64::DecodeError,
    anyhow::Error,
    String,
);

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self(message.to_owned())
    }
}

/// The value of a step, or the scenario's failure.
#[allow(clippy::panic)] // Cucumber fails a step by panicking: this is that one place.
pub fn check<T>(outcome: Outcome<T>) -> T {
    match outcome {
        Ok(value) => value,
        Err(failure) => panic!("{failure}"),
    }
}
