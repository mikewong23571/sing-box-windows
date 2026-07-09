use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use std::cmp::min;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{error, info, warn};

/// 构造 Clash API WebSocket 中继端点 URL（纯函数，便于单测）。
pub(crate) fn build_relay_endpoint(api_port: u16, endpoint: &str, token: &str) -> String {
    format!("ws://127.0.0.1:{}{}?token={}", api_port, endpoint, token)
}

/// 直接的事件发送器，不再使用WebSocket中继
/// 后端直接连接到sing-box API，然后将数据作为Tauri事件发送到前端
pub struct EventDirectRelay<Payload, RT: Runtime = tauri::Wry> {
    app_handle: AppHandle<RT>,
    pub(crate) endpoint: String,
    pub(crate) event_name: String,
    parser: Arc<dyn Fn(Value) -> Payload + Send + Sync>,
}

impl<Payload: Send + Sync + 'static + Serialize, RT: Runtime> EventDirectRelay<Payload, RT> {
    pub fn new<F>(
        app_handle: AppHandle<RT>,
        endpoint: &str,
        event_name: &str,
        parser: F,
        api_port: u16,
        token: String,
    ) -> Self
    where
        F: Fn(Value) -> Payload + Send + Sync + 'static,
    {
        Self {
            app_handle,
            endpoint: build_relay_endpoint(api_port, endpoint, &token),
            event_name: event_name.to_string(),
            parser: Arc::new(parser),
        }
    }

    /// 启动直接事件中继。
    ///
    /// 该 future 的生命周期必须跟随 websocket 读取循环，不能被一个空的发送任务提前结束；
    /// 否则前端会在内核仍运行时失去日志/连接/流量事件。
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.endpoint.as_str();
        let (ws_stream, _) = connect_async(url).await?;
        let (_write, mut read) = ws_stream.split();

        let mut message_count = 0u64;

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
                    Ok(data) => {
                        let parsed_data = (self.parser.as_ref())(data);

                        // 直接发送 Tauri 事件到前端
                        self.app_handle
                            .emit(&self.event_name, &parsed_data)
                            .map_err(|e| {
                                let message = format!("发送{}事件失败: {}", self.event_name, e);
                                error!("{}", message);
                                std::io::Error::new(std::io::ErrorKind::BrokenPipe, message)
                            })?;

                        message_count += 1;

                        // 每100条消息记录一次
                        if message_count % 100 == 0 {
                            info!("{} 已处理{}条数据", self.event_name, message_count);
                        }
                    }
                    Err(e) => {
                        warn!("解析{}数据失败: {}", self.event_name, e);
                    }
                },
                Ok(Message::Close(frame)) => {
                    let message = format!("{} websocket 连接已关闭: {:?}", self.event_name, frame);
                    warn!("{}", message);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        message,
                    )
                    .into());
                }
                Err(e) => {
                    error!("{} websocket 连接错误: {}", self.event_name, e);
                    return Err(e.into());
                }
                _ => {}
            }
        }

        let message = format!("{} websocket 数据流结束", self.event_name);
        warn!("{}", message);
        Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, message).into())
    }
}

/// 创建流量数据事件发送器
pub fn create_traffic_event_relay<RT: Runtime>(
    app_handle: AppHandle<RT>,
    api_port: u16,
    token: String,
) -> EventDirectRelay<Value, RT> {
    EventDirectRelay::new(
        app_handle,
        "/traffic",
        "traffic-data",
        |data| data,
        api_port,
        token,
    )
}

/// 创建内存数据事件发送器
pub fn create_memory_event_relay<RT: Runtime>(
    app_handle: AppHandle<RT>,
    api_port: u16,
    token: String,
) -> EventDirectRelay<Value, RT> {
    EventDirectRelay::new(
        app_handle,
        "/memory",
        "memory-data",
        |data| data,
        api_port,
        token,
    )
}

/// 创建日志事件发送器
pub fn create_log_event_relay<RT: Runtime>(
    app_handle: AppHandle<RT>,
    api_port: u16,
    token: String,
) -> EventDirectRelay<Value, RT> {
    EventDirectRelay::new(
        app_handle,
        "/logs",
        "log-data",
        |data| data,
        api_port,
        token,
    )
}

/// 创建连接事件发送器
pub fn create_connection_event_relay<RT: Runtime>(
    app_handle: AppHandle<RT>,
    api_port: u16,
    token: String,
) -> EventDirectRelay<Value, RT> {
    EventDirectRelay::new(
        app_handle,
        "/connections",
        "connections-data",
        |data| data,
        api_port,
        token,
    )
}

/// 事件中继失败后的退避延迟（纯逻辑）。
pub(crate) fn next_relay_retry_delay(
    retry_count: u32,
    current: std::time::Duration,
    max_retry_delay: std::time::Duration,
) -> std::time::Duration {
    if retry_count <= 3 {
        std::time::Duration::from_secs(retry_count as u64)
    } else {
        min(current * 2, max_retry_delay)
    }
}

/// 启动事件中继器并在失败时按退避策略重试
pub async fn start_event_relay_with_retry<RT: Runtime>(
    relay: EventDirectRelay<Value, RT>,
    relay_type: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut retry_count = 0;
    let mut retry_delay = std::time::Duration::from_secs(1);
    let max_retry_delay = std::time::Duration::from_secs(10);

    info!("🔌 开始启动{}事件中继器", relay_type);

    loop {
        match relay.start().await {
            Ok(_) => {
                info!("✅ {}事件中继器启动成功并正常结束", relay_type);
                break Ok(());
            }
            Err(e) => {
                retry_count += 1;
                retry_delay = next_relay_retry_delay(retry_count, retry_delay, max_retry_delay);

                warn!(
                    "⚠️ {}事件中继器失败，{}秒后重试 (第{}次): {}",
                    relay_type,
                    retry_delay.as_secs(),
                    retry_count,
                    e
                );

                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockAppEnv;
    use futures_util::SinkExt;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn build_relay_endpoint_formats_query() {
        let url = build_relay_endpoint(9090, "/traffic", "tok");
        assert_eq!(url, "ws://127.0.0.1:9090/traffic?token=tok");
        assert!(build_relay_endpoint(1, "logs", "").contains("ws://127.0.0.1:1logs?token="));
    }

    #[test]
    fn next_relay_retry_delay_policy() {
        let max = std::time::Duration::from_secs(10);
        assert_eq!(
            next_relay_retry_delay(1, std::time::Duration::from_secs(1), max),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            next_relay_retry_delay(3, std::time::Duration::from_secs(1), max),
            std::time::Duration::from_secs(3)
        );
        assert_eq!(
            next_relay_retry_delay(4, std::time::Duration::from_secs(3), max),
            std::time::Duration::from_secs(6)
        );
        assert_eq!(
            next_relay_retry_delay(10, std::time::Duration::from_secs(8), max),
            max
        );
    }

    #[test]
    fn create_all_relay_factories_with_mock_app() {
        let env = MockAppEnv::new();
        let h = env.handle();
        let t = create_traffic_event_relay(h.clone(), 19090, "a".into());
        let m = create_memory_event_relay(h.clone(), 19091, "b".into());
        let l = create_log_event_relay(h.clone(), 19092, "c".into());
        let c = create_connection_event_relay(h, 19093, "d".into());
        assert!(t.endpoint.contains("/traffic"));
        assert!(m.endpoint.contains("/memory"));
        assert!(l.endpoint.contains("/logs"));
        assert!(c.endpoint.contains("/connections"));
        assert_eq!(t.event_name, "traffic-data");
        assert_eq!(m.event_name, "memory-data");
        assert_eq!(l.event_name, "log-data");
        assert_eq!(c.event_name, "connections-data");
    }

    #[tokio::test]
    async fn event_relay_start_processes_text_and_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            // 有效 JSON
            ws.send(Message::Text(r#"{"up":1,"down":2}"#.into()))
                .await
                .unwrap();
            // 无效 JSON → warn 分支
            ws.send(Message::Text("not-json".into())).await.unwrap();
            // 再发多条以覆盖计数（不要求 100）
            for i in 0..3 {
                ws.send(Message::Text(format!(r#"{{"n":{i}}}"#).into()))
                    .await
                    .unwrap();
            }
            let _ = ws.close(None).await;
        });

        let env = MockAppEnv::new();
        let relay = create_traffic_event_relay(env.handle(), port, "t".into());
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), relay.start()).await;
        assert!(result.is_ok(), "relay should finish within timeout");
        let err = result.unwrap();
        // 正常关闭路径应返回 Err(ConnectionAborted) 或类似
        assert!(err.is_err());
        let _ = server.await;
    }

    #[tokio::test]
    async fn event_relay_start_fails_when_no_server() {
        let env = MockAppEnv::new();
        // 未监听端口
        let relay = create_memory_event_relay(env.handle(), 1, "".into());
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), relay.start()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
}
