use super::*;

#[test]
fn test_kernel_status_payload_running() {
    let payload = KernelStatusPayload::running();
    assert!(payload.process_running);
    assert!(payload.api_ready);
    assert!(payload.websocket_ready);
    assert!(payload.readiness.relay_ready);
}

#[test]
fn test_kernel_status_payload_stopped() {
    let payload = KernelStatusPayload::stopped();
    assert!(!payload.process_running);
    assert!(!payload.api_ready);
    assert!(!payload.websocket_ready);
    assert!(!payload.readiness.process_alive);
}

#[test]
fn test_kernel_status_payload_to_json() {
    let payload = KernelStatusPayload::new(
        true,
        false,
        true,
        KernelReadinessSnapshot {
            config_validated: Some(true),
            process_spawned: Some(true),
            process_alive: true,
            api_ready: false,
            relay_ready: true,
        },
        None,
    );
    let json = payload.to_json();
    assert_eq!(json["process_running"], true);
    assert_eq!(json["api_ready"], false);
    assert_eq!(json["websocket_ready"], true);
    assert_eq!(json["readiness"]["process_alive"], true);
}

#[test]
fn test_kernel_error_payload_contains_compat_and_structured_fields() {
    let payload = build_kernel_error_payload(
        "KERNEL_START_FAILED",
        "内核启动失败",
        Some("配置校验失败"),
        Some("kernel.runtime.start"),
        true,
    );

    assert_eq!(payload["code"], "KERNEL_START_FAILED");
    assert_eq!(payload["message"], "内核启动失败");
    assert_eq!(payload["details"], "配置校验失败");
    assert_eq!(payload["source"], "kernel.runtime.start");
    assert_eq!(payload["recoverable"], true);
    assert_eq!(payload["startup_diagnosis"]["kind"], "config_invalid");
    // 兼容老前端字段
    assert_eq!(payload["error"], "内核启动失败");
    assert!(payload["timestamp"].as_u64().is_some());
}

#[test]
fn test_build_kernel_error_payload_should_include_structured_api_http_error_diagnosis() {
    let payload = build_kernel_error_payload(
        "KERNEL_API_HTTP_ERROR",
        "内核 API 返回了错误状态码",
        Some("稳定性检查第4次失败：API状态码 400 Bad Request"),
        Some("kernel.runtime.startup_stability"),
        true,
    );

    assert_eq!(payload["startup_diagnosis"]["kind"], "api_http_error");
    assert_eq!(payload["startup_diagnosis"]["stage"], "readiness");
    assert_eq!(payload["startup_diagnosis"]["http_status"], 400);
}

#[test]
fn vec_sink_records_kernel_events() {
    let sink = VecSink::default();
    emit_kernel_starting_with_sink(&sink, "manual", 9090, 7890);
    let events = sink.events.lock().unwrap();
    assert!(events.iter().any(|(name, _)| name == "kernel-starting"));
    assert!(events.iter().any(|(name, _)| name == "kernel-status-changed"));
}
