use super::*;

#[test]
fn transition_planner_preserves_stopped_state_for_runtime_changes() {
    let desired = KernelDesiredState::Stopped;
    let observed = KernelObservedState::Stopped;

    assert_eq!(
        plan_kernel_transition(
            desired,
            observed,
            KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::PersistOnly),
        ),
        KernelAction::ApplyConfigOnly
    );
    assert_eq!(
        plan_kernel_transition(
            desired,
            observed,
            KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::HotApply),
        ),
        KernelAction::ApplyConfigOnly
    );
    assert_eq!(
        plan_kernel_transition(
            desired,
            observed,
            KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::RestartIfRunning),
        ),
        KernelAction::ApplyConfigOnly
    );
}

#[test]
fn transition_planner_restarts_only_running_desired_kernel() {
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Running,
            KernelObservedState::Running,
            KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::RestartIfRunning),
        ),
        KernelAction::Restart
    );
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Stopped,
            KernelObservedState::Running,
            KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::RestartIfRunning),
        ),
        KernelAction::ApplyConfigOnly
    );
}

#[test]
fn transition_planner_handles_explicit_intent_and_crashes() {
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Stopped,
            KernelObservedState::Stopped,
            KernelRequestKind::UserStart,
        ),
        KernelAction::Start
    );
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Stopped,
            KernelObservedState::Stopped,
            KernelRequestKind::UserRestart,
        ),
        KernelAction::Reject
    );
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Stopped,
            KernelObservedState::Crashed,
            KernelRequestKind::ProcessCrashed,
        ),
        KernelAction::Noop
    );
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Running,
            KernelObservedState::Crashed,
            KernelRequestKind::ProcessCrashed,
        ),
        KernelAction::Start
    );
}

#[test]
fn transition_planner_honors_startup_policy_and_shutdown() {
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Stopped,
            KernelObservedState::Stopped,
            KernelRequestKind::StartupReconcile { auto_start: false },
        ),
        KernelAction::Noop
    );
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Running,
            KernelObservedState::Stopped,
            KernelRequestKind::StartupReconcile { auto_start: true },
        ),
        KernelAction::Start
    );
    assert_eq!(
        plan_kernel_transition(
            KernelDesiredState::Running,
            KernelObservedState::Running,
            KernelRequestKind::Shutdown,
        ),
        KernelAction::Stop
    );
}

#[test]
fn test_kernel_state_transitions() {
    let manager = KernelStateManager::new();

    assert_eq!(manager.get_state(), KernelState::Stopped);
    assert!(manager.get_state().can_start());

    assert!(manager.try_transition_to_starting());
    assert_eq!(manager.get_state(), KernelState::Starting);
    assert!(!manager.get_state().can_start());

    manager.mark_running(12081);
    assert_eq!(manager.get_state(), KernelState::Running);
    assert!(manager.get_state().is_running());

    assert!(manager.try_transition_to_stopping());
    assert_eq!(manager.get_state(), KernelState::Stopping);

    manager.mark_stopped();
    assert_eq!(manager.get_state(), KernelState::Stopped);
}

#[test]
fn test_kernel_state_should_record_higher_priority_startup_diagnosis() {
    let manager = KernelStateManager::new();
    let attempt_id = manager.begin_attempt("test");

    manager.record_startup_diagnosis(StartupDiagnosis {
        attempt_id: attempt_id.clone(),
        stage: StartupStage::Readiness,
        code: "KERNEL_API_TIMEOUT".to_string(),
        kind: StartupDiagnosisKind::ApiTimeout,
        message: "api timeout".to_string(),
        detail: "timeout".to_string(),
        source: "kernel.runtime.startup_stability".to_string(),
        recoverable: true,
        config_path: None,
        http_status: None,
        suggested_actions: None,
        timestamp_ms: 1,
    });
    manager.record_startup_diagnosis(StartupDiagnosis {
        attempt_id,
        stage: StartupStage::Preflight,
        code: "KERNEL_CONFIG_INVALID".to_string(),
        kind: StartupDiagnosisKind::ConfigInvalid,
        message: "config invalid".to_string(),
        detail: "detail".to_string(),
        source: "kernel.runtime.preflight".to_string(),
        recoverable: true,
        config_path: None,
        http_status: None,
        suggested_actions: None,
        timestamp_ms: 2,
    });

    let diagnosis = manager
        .get_startup_diagnosis()
        .expect("should record diagnosis");
    assert_eq!(diagnosis.kind, StartupDiagnosisKind::ConfigInvalid);
    assert_eq!(diagnosis.code, "KERNEL_CONFIG_INVALID");
}
