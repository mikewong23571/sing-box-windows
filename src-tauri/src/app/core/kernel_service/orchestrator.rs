//! 内核生命周期编排器
//!
//! 通过单队列串行执行变更型操作，避免 start/stop/restart 并发竞态。

use futures::future::BoxFuture;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

type OperationResult = Result<Value, String>;
type OperationFuture = BoxFuture<'static, OperationResult>;
/// 将编排事件发出去的钩子（AppHandle 解耦，MockRuntime 可传 None）。
type EmitHook = Option<Box<dyn Fn(&str, Value) + Send>>;

struct OperationRequest {
    op_id: String,
    op_name: &'static str,
    emit: EmitHook,
    task: OperationFuture,
    response_tx: oneshot::Sender<OperationResult>,
}

const QUEUE_CAPACITY: usize = 32;

static OP_COUNTER: AtomicU64 = AtomicU64::new(1);
static STATE_VERSION: AtomicU64 = AtomicU64::new(0);
static CURRENT_OPERATION: Mutex<Option<(String, &'static str, u64)>> = Mutex::new(None);
/// 跨 `#[tokio::test]` runtime 时旧 receiver 可能已 drop，需可重建。
static ORCHESTRATOR_TX: Mutex<Option<mpsc::Sender<OperationRequest>>> = Mutex::new(None);

pub(crate) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn next_op_id() -> String {
    let ts = now_millis();
    let seq = OP_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("op-{}-{}", ts, seq)
}

pub(crate) fn bump_state_version() -> u64 {
    STATE_VERSION.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn current_state_version() -> u64 {
    STATE_VERSION.load(Ordering::SeqCst)
}

pub fn current_operation_meta() -> Option<(String, &'static str, u64)> {
    CURRENT_OPERATION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub(crate) fn with_operation_meta(
    mut value: Value,
    op_id: &str,
    op_name: &'static str,
    state_version: u64,
) -> Value {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("op_id".to_string(), json!(op_id));
        obj.insert("operation".to_string(), json!(op_name));
        obj.insert("state_version".to_string(), json!(state_version));
        value
    } else {
        json!({
            "success": true,
            "data": value,
            "op_id": op_id,
            "operation": op_name,
            "state_version": state_version
        })
    }
}

/// 构造编排事件 payload（纯逻辑）。
pub(crate) fn build_operation_event_payload(
    op_id: &str,
    op_name: &'static str,
    state_version: u64,
    error: Option<&str>,
) -> Value {
    json!({
        "op_id": op_id,
        "operation": op_name,
        "state_version": state_version,
        "timestamp": now_millis(),
        "error": error
    })
}

fn fire_emit(emit: &EmitHook, event: &str, payload: Value) {
    if let Some(hook) = emit {
        hook(event, payload);
    }
}

async fn run_worker(mut rx: mpsc::Receiver<OperationRequest>) {
    while let Some(req) = rx.recv().await {
        let state_version = bump_state_version();
        let queued_at = now_millis();
        *CURRENT_OPERATION
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some((req.op_id.clone(), req.op_name, state_version));

        fire_emit(
            &req.emit,
            "kernel-operation-started",
            build_operation_event_payload(&req.op_id, req.op_name, state_version, None),
        );

        info!(
            "内核编排器开始执行: {} (op_id={}, state_version={}, queued_at={})",
            req.op_name, req.op_id, state_version, queued_at
        );

        let result = req.task.await;
        let final_result = match result {
            Ok(value) => {
                fire_emit(
                    &req.emit,
                    "kernel-operation-finished",
                    build_operation_event_payload(&req.op_id, req.op_name, state_version, None),
                );
                Ok(with_operation_meta(
                    value,
                    &req.op_id,
                    req.op_name,
                    state_version,
                ))
            }
            Err(err) => {
                warn!(
                    "内核编排器执行失败: {} (op_id={}, err={})",
                    req.op_name, req.op_id, err
                );
                fire_emit(
                    &req.emit,
                    "kernel-operation-failed",
                    build_operation_event_payload(
                        &req.op_id,
                        req.op_name,
                        state_version,
                        Some(&err),
                    ),
                );
                Err(err)
            }
        };

        if req.response_tx.send(final_result).is_err() {
            warn!("内核编排器响应发送失败: {}", req.op_id);
        }
        let mut current = CURRENT_OPERATION
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if current
            .as_ref()
            .map(|(op_id, _, _)| op_id == &req.op_id)
            .unwrap_or(false)
        {
            *current = None;
        }
    }
}

fn get_sender() -> mpsc::Sender<OperationRequest> {
    let mut guard = ORCHESTRATOR_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(tx) = guard.as_ref() {
        if !tx.is_closed() {
            return tx.clone();
        }
    }
    let (tx, rx) = mpsc::channel::<OperationRequest>(QUEUE_CAPACITY);
    // 使用独立后台 runtime 承载 worker，避免绑定到短命的 test runtime
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("orchestrator worker runtime");
        rt.block_on(run_worker(rx));
    });
    *guard = Some(tx.clone());
    tx
}

async fn execute_kernel_operation_with_emit(
    emit: EmitHook,
    op_name: &'static str,
    task: OperationFuture,
) -> OperationResult {
    let op_id = next_op_id();
    let (resp_tx, resp_rx) = oneshot::channel();

    let request = OperationRequest {
        op_id: op_id.clone(),
        op_name,
        emit,
        task,
        response_tx: resp_tx,
    };

    let tx = get_sender();
    tx.send(request)
        .await
        .map_err(|e| format!("提交编排任务失败: {}", e))?;

    match resp_rx.await {
        Ok(result) => result,
        Err(e) => {
            error!("内核编排器响应异常 (op_id={}): {}", op_id, e);
            Err(format!("编排任务异常中断: {}", e))
        }
    }
}

/// 串行执行内核生命周期操作（任意 Runtime，事件通过 AppHandle::emit）。
pub async fn execute_kernel_operation<R: Runtime>(
    app_handle: AppHandle<R>,
    op_name: &'static str,
    task: OperationFuture,
) -> OperationResult {
    let emit: EmitHook = Some(Box::new(move |event, payload| {
        let _ = app_handle.emit(event, payload);
    }));
    execute_kernel_operation_with_emit(emit, op_name, task).await
}

/// 无 emit 的编排执行（单测 hermetic 路径，仍走同一串行队列）。
pub async fn execute_kernel_operation_emitless(
    op_name: &'static str,
    task: OperationFuture,
) -> OperationResult {
    execute_kernel_operation_with_emit(None, op_name, task).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn op_id_and_state_version_progress() {
        let a = next_op_id();
        let b = next_op_id();
        assert!(a.starts_with("op-"));
        assert_ne!(a, b);
        assert!(now_millis() > 0);
        let v0 = current_state_version();
        let v1 = bump_state_version();
        assert!(v1 > v0 || v1 >= 1);
        let _ = current_state_version();
    }

    #[test]
    fn with_operation_meta_object_and_non_object() {
        let base = json!({"success": true, "message": "ok"});
        let m = with_operation_meta(base, "op-1", "start", 3);
        assert_eq!(m["op_id"], "op-1");
        assert_eq!(m["operation"], "start");
        assert_eq!(m["state_version"], 3);

        let wrapped = with_operation_meta(json!("plain"), "op-2", "stop", 4);
        assert_eq!(wrapped["op_id"], "op-2");
        assert_eq!(wrapped["data"], "plain");
        assert_eq!(wrapped["success"], true);
    }

    #[test]
    fn build_operation_event_payload_fields() {
        let p = build_operation_event_payload("op-x", "kernel.start", 9, Some("boom"));
        assert_eq!(p["op_id"], "op-x");
        assert_eq!(p["operation"], "kernel.start");
        assert_eq!(p["state_version"], 9);
        assert_eq!(p["error"], "boom");
        assert!(p["timestamp"].as_u64().unwrap_or(0) > 0);

        let p2 = build_operation_event_payload("op-y", "kernel.stop", 1, None);
        assert!(p2["error"].is_null());
    }

    #[tokio::test]
    async fn execute_emitless_success_and_error() {
        let ok = execute_kernel_operation_emitless(
            "test.ok",
            Box::pin(async { Ok(json!({"success": true, "message": "hi"})) }),
        )
        .await
        .expect("ok op");
        assert_eq!(ok["success"], true);
        assert_eq!(ok["operation"], "test.ok");
        assert!(ok.get("op_id").is_some());
        assert!(ok.get("state_version").is_some());

        let err = execute_kernel_operation_emitless(
            "test.err",
            Box::pin(async { Err("fail-op".to_string()) }),
        )
        .await;
        assert!(err.unwrap_err().contains("fail-op"));
    }

    #[tokio::test]
    async fn execute_with_recording_emit_hook() {
        use std::sync::{Arc, Mutex};
        let events: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let events2 = events.clone();
        let emit: EmitHook = Some(Box::new(move |name, payload| {
            events2.lock().unwrap().push((name.to_string(), payload));
        }));

        let result = execute_kernel_operation_with_emit(
            emit,
            "test.emit",
            Box::pin(async { Ok(json!({"success": true})) }),
        )
        .await
        .unwrap();
        assert_eq!(result["operation"], "test.emit");

        let logged = events.lock().unwrap().clone();
        assert!(logged.iter().any(|(n, _)| n == "kernel-operation-started"));
        assert!(logged.iter().any(|(n, _)| n == "kernel-operation-finished"));
    }
}
