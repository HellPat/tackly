use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The most tasks one day's plan may hold. The app asks for fewer (3 by default).
pub const MAX_FOCUS: usize = 10;

/// One day's plan: the tasks someone will do, in their order, each once, at
/// most [`MAX_FOCUS`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Uuid>", into = "Vec<Uuid>")]
pub struct Focus(Vec<Uuid>);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("a day holds at most {MAX_FOCUS} tasks")]
pub struct TooMuch;

impl Focus {
    /// A task named twice counts once, at its first place.
    pub fn new(tasks: impl IntoIterator<Item = Uuid>) -> Result<Self, TooMuch> {
        let mut planned: Vec<Uuid> = Vec::new();
        for task in tasks {
            if !planned.contains(&task) {
                planned.push(task);
            }
        }
        if planned.len() > MAX_FOCUS {
            return Err(TooMuch);
        }
        Ok(Self(planned))
    }

    pub fn tasks(&self) -> &[Uuid] {
        &self.0
    }
}

impl TryFrom<Vec<Uuid>> for Focus {
    type Error = TooMuch;

    fn try_from(tasks: Vec<Uuid>) -> Result<Self, TooMuch> {
        Self::new(tasks)
    }
}

impl From<Focus> for Vec<Uuid> {
    fn from(focus: Focus) -> Self {
        focus.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u128) -> Vec<Uuid> {
        (1..=n).map(Uuid::from_u128).collect()
    }

    #[test]
    fn a_plan_keeps_its_order_and_names_a_task_once() {
        let plan = Focus::new([Uuid::from_u128(2), Uuid::from_u128(1), Uuid::from_u128(2)])
            .expect("a plan");
        assert_eq!(plan.tasks(), [Uuid::from_u128(2), Uuid::from_u128(1)]);
    }

    #[test]
    fn a_day_holds_a_limited_number_of_tasks() {
        assert!(Focus::new(ids(MAX_FOCUS as u128)).is_ok());
        assert_eq!(Focus::new(ids(MAX_FOCUS as u128 + 1)), Err(TooMuch));
    }
}
