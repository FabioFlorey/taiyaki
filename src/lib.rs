pub mod api;
pub mod authority;
pub mod db;
pub mod mcp;
pub mod planning;

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestRunStatus {
    Pending,
    Running,
    Passed,
    Failed,
    Skipped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRun {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: TestRunStatus,
    pub result: Option<String>,
}

impl TestRun {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            started_at: Utc::now(),
            ended_at: None,
            status: TestRunStatus::Pending,
            result: None,
        }
    }
}

impl Default for TestRun {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub runs: Vec<TestRun>,
}

impl Test {
    pub fn new(name: impl Into<String>, description: impl Into<String>, enabled: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            enabled,
            runs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSet {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub tests: Vec<Test>,
}

impl TestSet {
    pub fn new(name: impl Into<String>, description: impl Into<String>, enabled: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            enabled,
            tests: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtask {
    pub id: Uuid,
    pub name: String,
    pub description: String,
}

impl Subtask {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub subtasks: Vec<Subtask>,
}

impl Task {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            subtasks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub tasks: Vec<Task>,
}

impl Chapter {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            start_date,
            end_date,
            tasks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub chapters: Vec<Chapter>,
}

impl Project {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            start_date,
            end_date,
            chapters: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Chapter, Project, Subtask, Task, Test, TestRun, TestRunStatus, TestSet};
    use chrono::NaiveDate;

    #[test]
    fn creates_test_set() {
        let test_set = TestSet::new("Core tests", "Main test suite", true);

        assert_ne!(test_set.id.as_u128(), 0);
        assert_eq!(test_set.name, "Core tests");
        assert_eq!(test_set.description, "Main test suite");
        assert!(test_set.enabled);
        assert!(test_set.tests.is_empty());
    }

    #[test]
    fn creates_test_with_empty_run_list() {
        let test = Test::new("Health check", "Verifies the service is healthy", true);

        assert_ne!(test.id.as_u128(), 0);
        assert_eq!(test.name, "Health check");
        assert_eq!(test.description, "Verifies the service is healthy");
        assert!(test.enabled);
        assert!(test.runs.is_empty());
    }

    #[test]
    fn creates_pending_test_run() {
        let run = TestRun::new();

        assert_ne!(run.id.as_u128(), 0);
        assert_eq!(run.status, TestRunStatus::Pending);
        assert!(run.ended_at.is_none());
        assert!(run.result.is_none());
    }

    #[test]
    fn creates_project_with_uuid() {
        let project = Project::new(
            "Mimi",
            "Simple project",
            NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
        );

        assert_eq!(project.name, "Mimi");
        assert_eq!(project.description, "Simple project");
        assert_ne!(project.id.as_u128(), 0);
        assert!(project.chapters.is_empty());
    }

    #[test]
    fn creates_task_with_id() {
        let task = Task::new("First task", "Task description");

        assert_ne!(task.id.as_u128(), 0);
        assert_eq!(task.name, "First task");
        assert_eq!(task.description, "Task description");
        assert!(task.subtasks.is_empty());
    }

    #[test]
    fn creates_subtask_with_id() {
        let subtask = Subtask::new("First subtask", "Subtask description");

        assert_ne!(subtask.id.as_u128(), 0);
        assert_eq!(subtask.name, "First subtask");
        assert_eq!(subtask.description, "Subtask description");
    }

    #[test]
    fn creates_chapter_with_empty_task_list() {
        let chapter = Chapter::new(
            "Planning",
            "Initial planning chapter",
            NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
        );

        assert_eq!(chapter.name, "Planning");
        assert_eq!(chapter.description, "Initial planning chapter");
        assert_ne!(chapter.id.as_u128(), 0);
        assert!(chapter.tasks.is_empty());
    }
}
