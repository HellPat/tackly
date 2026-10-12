//! Problems with their solutions and steps, and each person's plan for a day.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    commands::FamilyCommand,
    errors::FamilyError,
    events::{DomainEvent, FamilyEvent, Solution},
    state::Family,
    tasks::Task,
};

/// Something that goes wrong at home, again and again, and how to fix it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    pub id: Uuid,
    pub title: String,
    /// A Material Symbols name.
    pub icon: String,
    pub solutions: Vec<Solution>,
    /// The last time it was noticed; kept when resolved, so Undo can reopen it.
    pub opening: Option<Opening>,
    pub open: bool,
}

/// One time a problem was noticed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    /// Who has it now.
    pub member_id: Uuid,
    /// Who noticed it (raised it, when that is someone else).
    pub by: Uuid,
    pub since: DateTime<Utc>,
}

impl Problem {
    /// Who has it, while it is open.
    pub fn holder(&self) -> Option<Uuid> {
        self.opening
            .as_ref()
            .filter(|_| self.open)
            .map(|opening| opening.member_id)
    }
}

impl Family {
    /// The open steps of an open problem (from the time it was last noticed).
    pub fn steps(&self, problem_id: Uuid) -> impl Iterator<Item = &Task> {
        let since = self
            .problems
            .get(&problem_id)
            .filter(|problem| problem.open)
            .and_then(|problem| problem.opening.as_ref())
            .map(|opening| opening.since);
        self.open_tasks().filter(move |task| {
            task.problem_id == Some(problem_id)
                && since.is_some_and(|since| task.created_at >= since)
        })
    }

    pub fn open_problems(&self) -> impl Iterator<Item = &Problem> {
        self.problems.values().filter(|problem| problem.open)
    }

    /// A member's plan for a day, as planned (`None`: not planned yet).
    pub fn plan(&self, member: Uuid, day: NaiveDate) -> Option<&[Uuid]> {
        self.plans
            .get(&member)
            .and_then(|days| days.get(&day))
            .map(Vec::as_slice)
    }

    /// What a member still has to do of a day's plan, in their order.
    pub fn focus(&self, member: Uuid, day: NaiveDate) -> Vec<&Task> {
        self.plan(member, day)
            .unwrap_or_default()
            .iter()
            .filter_map(|id| self.tasks.get(id))
            .filter(|task| !task.is_done() && task.assignee == Some(member))
            .collect()
    }

    pub(super) fn problem(&self, problem_id: Uuid) -> Result<&Problem, FamilyError> {
        self.problems
            .get(&problem_id)
            .ok_or(FamilyError::UnknownProblem)
    }

    /// Checks the problems commands and returns the events they produce, as (subject, event).
    pub(super) fn decide_problems(
        &self,
        command: FamilyCommand,
        me: Uuid,
    ) -> Result<Vec<(Uuid, DomainEvent)>, FamilyError> {
        use DomainEvent as E;
        use FamilyCommand as C;
        let one = |subject, event| Ok(vec![(subject, event)]);
        match command {
            C::CreateProblem {
                problem_id,
                title,
                icon,
                solutions,
            } => {
                let mut events = vec![(problem_id, E::ProblemCreated { title, icon })];
                for solution in solutions {
                    events.push((problem_id, E::ProblemSolutionAdded { solution }));
                }
                Ok(events)
            }
            C::RenameProblem { problem_id, title } => {
                self.problem(problem_id)?;
                one(problem_id, E::ProblemRenamed { title })
            }
            C::AddSolution {
                problem_id,
                solution,
                task_id,
            } => {
                let problem = self.problem(problem_id)?;
                let mut events = vec![];
                if let Some(holder) = problem.holder() {
                    events.extend(step(problem_id, &solution, task_id, holder));
                }
                events.insert(0, (problem_id, E::ProblemSolutionAdded { solution }));
                Ok(events)
            }
            C::RemoveSolution {
                problem_id,
                solution_id,
            } => {
                self.problem(problem_id)?;
                one(problem_id, E::ProblemSolutionRemoved { solution_id })
            }
            C::NoticeProblem {
                problem_id,
                member_id,
                task_ids,
            } => {
                let problem = self.problem(problem_id)?;
                if problem.open {
                    return Err(FamilyError::ProblemOpen);
                }
                self.members
                    .get(&member_id)
                    .ok_or(FamilyError::UnknownMember)?;
                if task_ids.len() != problem.solutions.len() {
                    return Err(FamilyError::WrongSteps);
                }
                let mut events = vec![(problem_id, E::ProblemNoticed { member_id })];
                for (solution, task_id) in problem.solutions.iter().zip(task_ids) {
                    events.extend(step(problem_id, solution, task_id, member_id));
                }
                Ok(events)
            }
            C::AssignProblem {
                problem_id,
                member_id,
            } => {
                if !self.problem(problem_id)?.open {
                    return Err(FamilyError::ProblemNotOpen);
                }
                self.members
                    .get(&member_id)
                    .ok_or(FamilyError::UnknownMember)?;
                let mut events = vec![(problem_id, E::ProblemAssigned { member_id })];
                for task in self.steps(problem_id) {
                    if task.assignee != Some(member_id) {
                        events.push((
                            task.id,
                            E::TaskAssigned {
                                member_id: Some(member_id),
                            },
                        ));
                    }
                }
                Ok(events)
            }
            C::ResolveProblem { problem_id } => {
                if !self.problem(problem_id)?.open {
                    return Err(FamilyError::ProblemNotOpen);
                }
                one(problem_id, E::ProblemResolved)
            }
            C::ReopenProblem { problem_id } => {
                let problem = self.problem(problem_id)?;
                if problem.open {
                    return Err(FamilyError::ProblemOpen);
                }
                if problem.opening.is_none() {
                    return Err(FamilyError::ProblemNotOpen);
                }
                one(problem_id, E::ProblemReopened)
            }
            C::PlanDay { day, task_ids } => {
                let mut events = vec![];
                for id in task_ids.tasks() {
                    let task = self.my_open_task(*id, me)?;
                    if task.assignee.is_none() {
                        events.push((
                            *id,
                            E::TaskAssigned {
                                member_id: Some(me),
                            },
                        ));
                    }
                }
                events.push((me, E::DayPlanned { day, task_ids }));
                Ok(events)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Applies the problems events; `false` when the event is not one of them.
    pub(super) fn apply_problems(&mut self, envelope: &FamilyEvent) -> bool {
        let (subject, by, at) = (
            envelope.subject_id,
            envelope.origin_device_id,
            envelope.occurred_at,
        );
        match &envelope.event {
            DomainEvent::ProblemCreated { title, icon } => {
                self.problems.entry(subject).or_insert(Problem {
                    id: subject,
                    title: title.to_string(),
                    icon: icon.to_string(),
                    solutions: Vec::new(),
                    opening: None,
                    open: false,
                });
            }
            DomainEvent::ProblemRenamed { title } => {
                if let Some(problem) = self.problems.get_mut(&subject) {
                    problem.title = title.to_string();
                }
            }
            DomainEvent::ProblemSolutionAdded { solution } => {
                if let Some(problem) = self.problems.get_mut(&subject)
                    && !problem
                        .solutions
                        .iter()
                        .any(|known| known.id == solution.id)
                {
                    problem.solutions.push(solution.clone());
                }
            }
            DomainEvent::ProblemSolutionRemoved { solution_id } => {
                if let Some(problem) = self.problems.get_mut(&subject) {
                    problem
                        .solutions
                        .retain(|solution| solution.id != *solution_id);
                }
            }
            DomainEvent::ProblemNoticed { member_id } => {
                if let Some(problem) = self.problems.get_mut(&subject)
                    && !problem.open
                {
                    problem.open = true;
                    problem.opening = Some(Opening {
                        member_id: *member_id,
                        by,
                        since: at,
                    });
                }
            }
            DomainEvent::ProblemAssigned { member_id } => {
                if let Some(opening) = self
                    .problems
                    .get_mut(&subject)
                    .filter(|problem| problem.open)
                    .and_then(|problem| problem.opening.as_mut())
                {
                    opening.member_id = *member_id;
                }
            }
            DomainEvent::ProblemResolved => {
                if let Some(problem) = self.problems.get_mut(&subject) {
                    problem.open = false;
                }
            }
            DomainEvent::ProblemReopened => {
                if let Some(problem) = self.problems.get_mut(&subject)
                    && problem.opening.is_some()
                {
                    problem.open = true;
                }
            }
            DomainEvent::DayPlanned { day, task_ids } => {
                self.plans
                    .entry(by)
                    .or_default()
                    .insert(*day, task_ids.tasks().to_vec());
            }
            _ => return false,
        }
        true
    }
}

/// A solution becomes a task of its own, for whoever has the problem.
pub(super) fn step(
    problem_id: Uuid,
    solution: &Solution,
    task_id: Uuid,
    holder: Uuid,
) -> [(Uuid, DomainEvent); 2] {
    [
        (
            task_id,
            DomainEvent::TaskCreated {
                list_id: None,
                title: solution.title.clone(),
                place_ids: vec![],
                problem_id: Some(problem_id),
            },
        ),
        (
            task_id,
            DomainEvent::TaskAssigned {
                member_id: Some(holder),
            },
        ),
    ]
}
