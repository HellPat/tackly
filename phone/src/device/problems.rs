//! Problems (things that go wrong at home, with their solutions) and plans
//! (each person's focus for a day).

use anyhow::{Context, Result};
use chrono::NaiveDate;
use tackly_domain::{FamilyCommand, Focus, Solution};
use uuid::Uuid;

use super::Device;

impl Device {
    // ---- problems --------------------------------------------------------------------------

    /// A problem the family knows from now on, with its solutions.
    pub async fn create_problem(
        &mut self,
        title: &str,
        icon: &str,
        solutions: &[&str],
    ) -> Result<Uuid> {
        let problem_id = Uuid::now_v7();
        self.run(FamilyCommand::CreateProblem {
            problem_id,
            title: title.parse()?,
            icon: icon.parse()?,
            solutions: solutions
                .iter()
                .map(|title| {
                    Ok(Solution {
                        id: Uuid::now_v7(),
                        title: title.parse()?,
                    })
                })
                .collect::<Result<_>>()?,
        })
        .await?;
        Ok(problem_id)
    }

    pub async fn rename_problem(&mut self, problem_id: Uuid, title: &str) -> Result<()> {
        self.run(FamilyCommand::RenameProblem {
            problem_id,
            title: title.parse()?,
        })
        .await
    }

    /// While the problem is open, the solution is a step for whoever has it right away.
    pub async fn add_solution(&mut self, problem_id: Uuid, title: &str) -> Result<()> {
        self.run(FamilyCommand::AddSolution {
            problem_id,
            solution: Solution {
                id: Uuid::now_v7(),
                title: title.parse()?,
            },
            task_id: Uuid::now_v7(),
        })
        .await
    }

    pub async fn remove_solution(&mut self, problem_id: Uuid, solution_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::RemoveSolution {
            problem_id,
            solution_id,
        })
        .await
    }

    /// Noticed for yourself (`member_id` is this phone) or raised for someone else.
    pub async fn notice_problem(&mut self, problem_id: Uuid, member_id: Uuid) -> Result<()> {
        let solutions = self
            .state()?
            .problems
            .get(&problem_id)
            .map(|problem| problem.solutions.len())
            .context("unknown problem")?;
        self.run(FamilyCommand::NoticeProblem {
            problem_id,
            member_id,
            task_ids: (0..solutions).map(|_| Uuid::now_v7()).collect(),
        })
        .await
    }

    pub async fn assign_problem(&mut self, problem_id: Uuid, member_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::AssignProblem {
            problem_id,
            member_id,
        })
        .await
    }

    pub async fn resolve_problem(&mut self, problem_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::ResolveProblem { problem_id }).await
    }

    /// Undo of resolving: open again, with the same steps.
    pub async fn reopen_problem(&mut self, problem_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::ReopenProblem { problem_id }).await
    }

    // ---- plans -----------------------------------------------------------------------------

    /// My focus for a day, in order; free tasks become mine. Replaces an earlier plan.
    pub async fn plan_day(&mut self, day: NaiveDate, task_ids: &[Uuid]) -> Result<()> {
        self.run(FamilyCommand::PlanDay {
            day,
            task_ids: Focus::new(task_ids.iter().copied())?,
        })
        .await
    }
}
