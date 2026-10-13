//! Work-item orchestration. The model may request work and submit outcomes, but
//! cannot issue PASS receipts or transition itself to DONE.
use crate::planning::{Disposition, Plan, Priority, ReviewState, WorkItem, WorkItemKind};
use chrono::{DateTime, Utc};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Ready,
    Implementing,
    Verifying,
    Done,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestType {
    Functional,
    Integration,
    Regression,
    Security,
    Performance,
    QualityOfLife,
    Accessibility,
    Reliability,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    #[default]
    NotRun,
    Running,
    Pass,
    Fail,
    Error,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestGate {
    pub test_type: TestType,
    pub test_id: String,
    pub test_name: String,
    pub test_description: String,
    pub test_goal: String,
    pub precondition: String,
    pub expected_result: String,
    /// Test source or reference submitted by the author. NEVER executed from
    /// an untrusted MCP payload as a command.
    pub test_code: String,
    /// Authority-managed. Submission cannot select PASS.
    #[serde(default)]
    pub test_status: TestStatus,
}
impl TestGate {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("test_id", &self.test_id),
            ("test_name", &self.test_name),
            ("test_description", &self.test_description),
            ("test_goal", &self.test_goal),
            ("precondition", &self.precondition),
            ("expected_result", &self.expected_result),
            ("test_code", &self.test_code),
        ] {
            if value.trim().is_empty()
                || value.len() > if name == "test_code" { 8192 } else { 1024 }
            {
                return Err(format!(
                    "{name} must be nonempty and within its length limit"
                ));
            }
        }
        if self.test_id.len() > 128
            || !self
                .test_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return Err("test ID must be a simple identifier".into());
        }
        if self.test_status != TestStatus::NotRun {
            return Err("test status may not be supplied by the author".into());
        }
        Ok(())
    }
}

/// Trusted only when received from the independently authenticated verifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateObservation {
    pub test_id: String,
    pub test_code_sha256: String,
    pub code_valid: bool,
    pub passed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateReceipt {
    pub test_id: String,
    pub work_item_id: Uuid,
    pub plan_revision: u64,
    pub source_tree_sha256: String,
    pub test_code_sha256: String,
    pub passed: bool,
    pub observed_at: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkRecord {
    pub id: Uuid,
    pub phase: Phase,
    pub assigned_to: Option<String>,
    /// An opaque capability: used to report work, never to mark PASS.
    pub lease: Option<Uuid>,
    pub attempt: u32,
    pub feedback: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub plan_id: Uuid,
    pub plan_revision: u64,
    pub revision: u64,
    pub work: BTreeMap<Uuid, WorkRecord>,
    pub gates: BTreeMap<String, TestGate>,
    #[serde(default)]
    pub receipts: BTreeMap<Uuid, Vec<GateReceipt>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub work_item: WorkItem,
    pub lease: Uuid,
    pub workflow_revision: u64,
    pub instructions: String,
}
impl Workflow {
    pub fn new(plan: &Plan) -> Result<Self, String> {
        if plan.review_state() != &ReviewState::Submitted {
            return Err("submit the plan for review before activating work".into());
        }
        let work = plan
            .work_items()
            .iter()
            .map(|item| {
                (
                    item.id,
                    WorkRecord {
                        id: item.id,
                        phase: if item.disposition == Disposition::Blocked {
                            Phase::Blocked
                        } else {
                            Phase::Ready
                        },
                        assigned_to: None,
                        lease: None,
                        attempt: 0,
                        feedback: None,
                    },
                )
            })
            .collect();
        Ok(Self {
            plan_id: plan.id(),
            plan_revision: plan.revision(),
            revision: 1,
            work,
            gates: BTreeMap::new(),
            receipts: BTreeMap::new(),
        })
    }
    pub fn validate_against(&self, plan: &Plan) -> Result<(), String> {
        if self.plan_id != plan.id()
            || self.plan_revision != plan.revision()
            || plan.review_state() != &ReviewState::Submitted
        {
            return Err("workflow is not bound to the current submitted plan".into());
        }
        let expected: HashSet<Uuid> = plan.work_items().iter().map(|i| i.id).collect();
        if expected != self.work.keys().copied().collect() {
            return Err("workflow/work-item mismatch".into());
        }
        if self.revision == 0 {
            return Err("invalid workflow revision".into());
        }
        Ok(())
    }
    fn bump(&mut self) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("workflow revision overflow")?;
        Ok(())
    }
    pub fn define_gate(&mut self, mut gate: TestGate) -> Result<(), String> {
        gate.validate()?;
        if self
            .work
            .values()
            .any(|record| !matches!(record.phase, Phase::Ready | Phase::Blocked))
        {
            return Err("test gates are frozen after work is first assigned".into());
        }
        if self.gates.contains_key(&gate.test_id) {
            return Err("duplicate Test ID".into());
        }
        if self.gates.len() >= 1024 {
            return Err("too many gates".into());
        }
        gate.test_status = TestStatus::NotRun;
        self.gates.insert(gate.test_id.clone(), gate);
        self.bump()
    }
    pub fn missing_gates(&self, item: &WorkItem) -> Vec<String> {
        item.required_gate_ids
            .iter()
            .filter(|id| !self.gates.contains_key(*id))
            .cloned()
            .collect()
    }
    pub fn next(
        &mut self,
        plan: &Plan,
        agent: &str,
        at: DateTime<Utc>,
    ) -> Result<Option<Assignment>, String> {
        self.validate_against(plan)?;
        if agent.trim().is_empty() || agent.len() > 128 {
            return Err("invalid agent label".into());
        }
        // Only leaf tasks are assigned. Parent milestones complete by children.
        let parents: HashSet<Uuid> = plan
            .work_items()
            .iter()
            .filter_map(|i| i.parent_id)
            .collect();
        let mut available: Vec<&WorkItem> = plan
            .work_items()
            .iter()
            .filter(|item| {
                !parents.contains(&item.id)
                    && item.kind == WorkItemKind::Task
                    && item.disposition == Disposition::Ready
                    && item.schedule.not_before.is_none_or(|start| start <= at)
                    && self.work[&item.id].phase == Phase::Ready
                    && item
                        .depends_on
                        .iter()
                        .all(|dep| self.work.get(dep).is_some_and(|w| w.phase == Phase::Done))
                    && self.missing_gates(item).is_empty()
            })
            .collect();
        let weight = |p: Priority| match p {
            Priority::Critical => 0,
            Priority::High => 1,
            Priority::Normal => 2,
            Priority::Low => 3,
        };
        available.sort_by_key(|item| (weight(item.priority), item.id));
        let Some(item) = available.first() else {
            return Ok(None);
        };
        let lease = Uuid::new_v4();
        let rec = self.work.get_mut(&item.id).expect("validated work exists");
        rec.phase = Phase::Implementing;
        rec.lease = Some(lease);
        rec.assigned_to = Some(agent.into());
        rec.attempt = rec.attempt.checked_add(1).ok_or("attempt overflow")?;
        self.bump()?;
        Ok(Some(Assignment{
            work_item:(*item).clone(),lease,workflow_revision:self.revision,
            instructions:"Implement the work item and its acceptance criteria. Submit an implementation report; only the authority can verify gates and mark DONE.".into()
        }))
    }
    pub fn report(&mut self, id: Uuid, lease: Uuid, notes: String) -> Result<(), String> {
        let rec = self.work.get_mut(&id).ok_or("unknown work item")?;
        if rec.phase != Phase::Implementing || rec.lease != Some(lease) {
            return Err("not the active implementation lease".into());
        }
        if notes.trim().is_empty() || notes.len() > 8192 {
            return Err("invalid implementation report".into());
        }
        rec.phase = Phase::Verifying;
        rec.feedback = Some(notes);
        rec.lease = None;
        self.bump()
    }
    pub fn fail(&mut self, id: Uuid, feedback: String) -> Result<(), String> {
        let rec = self.work.get_mut(&id).ok_or("unknown work item")?;
        if rec.phase != Phase::Verifying {
            return Err("item is not verifying".into());
        }
        if feedback.trim().is_empty() || feedback.len() > 8192 {
            return Err("invalid failure report".into());
        }
        rec.phase = Phase::Ready;
        rec.feedback = Some(feedback);
        for gate in self.gates.values_mut() {
            gate.test_status = TestStatus::NotRun;
        }
        self.bump()
    }
    fn close_parents(&mut self, plan: &Plan) {
        // Cascaded aggregation only. Parent with its own gates must be
        // independently verified after its descendants complete.
        for _ in 0..plan.work_items().len() {
            let mut advanced = false;
            for parent in plan.work_items() {
                let children: Vec<_> = plan
                    .work_items()
                    .iter()
                    .filter(|item| item.parent_id == Some(parent.id))
                    .collect();
                if children.is_empty()
                    || !children.iter().all(|item| {
                        self.work
                            .get(&item.id)
                            .is_some_and(|rec| rec.phase == Phase::Done)
                    })
                {
                    continue;
                }
                if !parent.depends_on.iter().all(|id| {
                    self.work
                        .get(id)
                        .is_some_and(|rec| rec.phase == Phase::Done)
                }) {
                    continue;
                }
                let rec = self.work.get_mut(&parent.id).expect("validated plan");
                if rec.phase == Phase::Ready {
                    rec.phase = if parent.required_gate_ids.is_empty() {
                        Phase::Done
                    } else {
                        Phase::Verifying
                    };
                    rec.feedback = Some("All child work items completed".into());
                    advanced = true;
                }
            }
            if !advanced {
                break;
            }
        }
    }

    /// Only the independent verifier may attest the current source snapshot.
    /// Any change invalidates previously completed work and downstream leases.
    pub(crate) fn refresh_source(
        &mut self,
        plan: &Plan,
        observed: Option<String>,
    ) -> Result<bool, String> {
        self.validate_against(plan)?;
        if let Some(tree) = &observed {
            if tree.len() != 64 || !tree.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err("invalid attested tree digest".into());
            }
        }
        let mut invalid: HashSet<Uuid> = self
            .work
            .iter()
            .filter_map(|(id, record)| {
                if record.phase == Phase::Done
                    && self.receipts.get(id).is_some_and(|items| {
                        items
                            .iter()
                            .any(|r| observed.as_deref() != Some(r.source_tree_sha256.as_str()))
                    })
                {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        let mut changed = true;
        while changed {
            changed = false;
            for item in plan.work_items() {
                if !invalid.contains(&item.id)
                    && (item.depends_on.iter().any(|dep| invalid.contains(dep))
                        || plan.work_items().iter().any(|child| {
                            child.parent_id == Some(item.id) && invalid.contains(&child.id)
                        }))
                {
                    invalid.insert(item.id);
                    changed = true;
                }
            }
        }
        if invalid.is_empty() {
            return Ok(false);
        }
        for id in invalid {
            let rec = self.work.get_mut(&id).ok_or("missing WorkItem")?;
            rec.phase = Phase::Ready;
            rec.lease = None;
            rec.feedback = Some("Source snapshot or dependency changed; prior PASS invalid".into());
            if let Some(old) = self.receipts.get(&id) {
                for receipt in old {
                    if let Some(gate) = self.gates.get_mut(&receipt.test_id) {
                        gate.test_status = TestStatus::NotRun;
                    }
                }
            }
        }
        self.bump()?;
        Ok(true)
    }

    /// Called only after an independently authenticated verifier process
    /// reports the execution observations for all required gates.
    pub(crate) fn observe(
        &mut self,
        plan: &Plan,
        id: Uuid,
        source_tree_sha256: String,
        results: Vec<GateObservation>,
    ) -> Result<(), String> {
        self.validate_against(plan)?;
        if source_tree_sha256.len() != 64
            || !source_tree_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid source tree digest".into());
        }
        let item = plan.get_item(id).ok_or("unknown work item")?;
        if self.work[&id].phase != Phase::Verifying {
            return Err("item is not verifying".into());
        }
        if item
            .depends_on
            .iter()
            .any(|dep| self.work.get(dep).is_none_or(|w| w.phase != Phase::Done))
        {
            return Err("work item dependencies are no longer verified".into());
        }
        if item.required_gate_ids.is_empty() {
            return Err("item has no gates".into());
        }
        if results.len() != item.required_gate_ids.len()
            || results
                .iter()
                .map(|v| &v.test_id)
                .collect::<HashSet<_>>()
                .len()
                != results.len()
            || item
                .required_gate_ids
                .iter()
                .any(|g| !results.iter().any(|r| r.test_id == *g))
        {
            return Err("verifier must report exactly every required gate".into());
        }
        let all_pass = results.iter().all(|r| {
            r.code_valid
                && r.passed
                && r.test_code_sha256.len() == 64
                && r.test_code_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        });
        let mut receipts = Vec::with_capacity(results.len());
        for result in results {
            let gate = self.gates.get_mut(&result.test_id).ok_or("unknown gate")?;
            gate.test_status = if result.code_valid && result.passed {
                TestStatus::Pass
            } else {
                TestStatus::Fail
            };
            receipts.push(GateReceipt {
                test_id: result.test_id,
                work_item_id: id,
                plan_revision: self.plan_revision,
                source_tree_sha256: source_tree_sha256.clone(),
                test_code_sha256: result.test_code_sha256,
                passed: result.passed && result.code_valid,
                observed_at: Utc::now(),
            });
        }
        self.receipts.insert(id, receipts);
        let rec = self.work.get_mut(&id).unwrap();
        rec.phase = if all_pass { Phase::Done } else { Phase::Ready };
        rec.feedback = Some(if all_pass {
            "Independent verifier PASS".into()
        } else {
            "Verification failed: inspect gate receipts and retry".into()
        });
        if all_pass {
            self.close_parents(plan);
        }
        self.bump()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{PlanChange, WorkItem};
    #[test]
    fn orders_by_dependency_then_priority_and_rejects_fake_completion() {
        let time = Utc::now();
        let mut plan = Plan::new("Plan", "", time).unwrap();
        let mut a = WorkItem::new("Foundation", "", time);
        a.disposition = Disposition::Ready;
        a.acceptance_criteria.push("test".into());
        a.required_gate_ids.push("foundation".into());
        let mut b = WorkItem::new("Depends", "", time);
        b.disposition = Disposition::Ready;
        b.priority = Priority::Critical;
        b.acceptance_criteria.push("test".into());
        b.required_gate_ids.push("dependent".into());
        b.depends_on.push(a.id);
        plan.apply_batch(
            1,
            time,
            vec![
                PlanChange::Add(b.clone()),
                PlanChange::Add(a.clone()),
                PlanChange::SubmitForReview,
            ],
        )
        .unwrap();
        let mut flow = Workflow::new(&plan).unwrap();
        for id in ["foundation", "dependent"] {
            flow.define_gate(TestGate {
                test_type: TestType::Functional,
                test_id: id.into(),
                test_name: id.into(),
                test_description: "Real testcase".into(),
                test_goal: "Test behavior".into(),
                precondition: "Configured harness".into(),
                expected_result: "Success".into(),
                test_code: "tests/functional.rs".into(),
                test_status: TestStatus::NotRun,
            })
            .unwrap();
        }
        let first = flow.next(&plan, "builder", time).unwrap().unwrap();
        assert_eq!(first.work_item.id, a.id);
        assert!(flow.next(&plan, "builder", time).unwrap().is_none());
        assert!(flow
            .report(a.id, Uuid::new_v4(), "implemented".into())
            .is_err());
        flow.report(a.id, first.lease, "implemented".into())
            .unwrap();
        assert!(flow.next(&plan, "builder", time).unwrap().is_none());
        assert!(flow.observe(&plan, a.id, "c".repeat(64), vec![]).is_err());
        flow.observe(
            &plan,
            a.id,
            "c".repeat(64),
            vec![GateObservation {
                test_id: "foundation".into(),
                test_code_sha256: "e".repeat(64),
                code_valid: true,
                passed: true,
            }],
        )
        .unwrap();
        let second = flow.next(&plan, "builder", time).unwrap().unwrap();
        assert_eq!(second.work_item.id, b.id);
    }
    #[test]
    fn parent_milestone_closes_only_after_verified_child_and_reopens_on_tree_change() {
        let at = Utc::now();
        let mut plan = Plan::new("Nested", "", at).unwrap();
        let mut parent = WorkItem::new("Milestone", "", at);
        parent.kind = WorkItemKind::Milestone;
        parent.disposition = Disposition::Ready;
        let mut child = WorkItem::new("Implement", "", at);
        child.parent_id = Some(parent.id);
        child.disposition = Disposition::Ready;
        child.required_gate_ids.push("test-real".into());
        child
            .acceptance_criteria
            .push("executed verifier passes".into());
        plan.apply_batch(
            1,
            at,
            vec![
                PlanChange::Add(child.clone()),
                PlanChange::Add(parent.clone()),
                PlanChange::SubmitForReview,
            ],
        )
        .unwrap();
        let mut workflow = Workflow::new(&plan).unwrap();
        workflow
            .define_gate(TestGate {
                test_type: TestType::Functional,
                test_id: "test-real".into(),
                test_name: "Real".into(),
                test_description: "Checks behavior".into(),
                test_goal: "detects regressions".into(),
                precondition: "reviewed source".into(),
                expected_result: "PASS".into(),
                test_code: "tests/test_real.rs".into(),
                test_status: TestStatus::NotRun,
            })
            .unwrap();
        let assigned = workflow.next(&plan, "agent", at).unwrap().unwrap();
        assert_eq!(assigned.work_item.id, child.id);
        assert_eq!(workflow.work[&parent.id].phase, Phase::Ready);
        workflow
            .report(child.id, assigned.lease, "source submitted".into())
            .unwrap();
        workflow
            .observe(
                &plan,
                child.id,
                "a".repeat(64),
                vec![GateObservation {
                    test_id: "test-real".into(),
                    test_code_sha256: "c".repeat(64),
                    code_valid: true,
                    passed: true,
                }],
            )
            .unwrap();
        assert_eq!(workflow.work[&child.id].phase, Phase::Done);
        assert_eq!(workflow.work[&parent.id].phase, Phase::Done);
        assert!(!workflow
            .refresh_source(&plan, Some("a".repeat(64)))
            .unwrap());
        assert!(workflow
            .refresh_source(&plan, Some("b".repeat(64)))
            .unwrap());
        assert_eq!(workflow.work[&child.id].phase, Phase::Ready);
        assert_eq!(workflow.work[&parent.id].phase, Phase::Ready);
        assert_eq!(workflow.gates["test-real"].test_status, TestStatus::NotRun);
    }

    #[test]
    fn gate_is_not_pass_by_author_submission() {
        let gate = TestGate {
            test_type: TestType::QualityOfLife,
            test_id: "QoL-1".into(),
            test_name: "UI".into(),
            test_description: "description".into(),
            test_goal: "goal".into(),
            precondition: "pre".into(),
            expected_result: "expected".into(),
            test_code: "assert!(true)".into(),
            test_status: TestStatus::Pass,
        };
        assert!(gate.validate().is_err());
    }
}
