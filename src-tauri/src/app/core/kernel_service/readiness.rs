use crate::app::core::kernel_service::status::is_kernel_running;
use crate::utils::http_client;
use std::time::Duration;
use tracing::info;

pub(crate) fn classify_startup_stability_failure(detail: &str) -> (&'static str, &'static str) {
    if detail.contains("API status") {
        (
            "KERNEL_API_HTTP_ERROR",
            "kernel API returned error status code",
        )
    } else if detail.contains("exited immediately") {
        (
            "KERNEL_PROCESS_EXITED_EARLY",
            "kernel process exited shortly after startup",
        )
    } else {
        (
            "KERNEL_API_TIMEOUT",
            "kernel API not ready within stability window",
        )
    }
}

/// 稳定性检查参数（测试可收紧以加速）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct StabilityCheckConfig {
    pub max_checks: u8,
    pub initial_retry_interval_ms: u64,
    pub max_retry_interval_ms: u64,
    pub api_timeout_ms: u64,
}

impl Default for StabilityCheckConfig {
    fn default() -> Self {
        Self {
            max_checks: 10,
            initial_retry_interval_ms: 300,
            max_retry_interval_ms: 2000,
            api_timeout_ms: 1000,
        }
    }
}

pub(crate) async fn verify_kernel_startup_stability(api_port: u16) -> Result<(), String> {
    verify_kernel_startup_stability_with_config(api_port, StabilityCheckConfig::default()).await
}

pub(crate) async fn verify_kernel_startup_stability_with_config(
    api_port: u16,
    cfg: StabilityCheckConfig,
) -> Result<(), String> {
    let client = http_client::get_client();
    let api_url = format!("http://127.0.0.1:{}/version", api_port);
    let mut last_error = String::new();

    for attempt in 1..=cfg.max_checks {
        if !is_kernel_running().await.unwrap_or(false) {
            return Err("kernel process exited immediately after startup".to_string());
        }

        match client
            .get(&api_url)
            .timeout(Duration::from_millis(cfg.api_timeout_ms))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                info!(
                    "kernel stability check passed (attempt {}/{})",
                    attempt, cfg.max_checks
                );
                return Ok(());
            }
            Ok(response) => {
                last_error = format!(
                    "stability check attempt {} failed: API status {}",
                    attempt,
                    response.status()
                );
            }
            Err(e) => {
                last_error = format!(
                    "stability check attempt {} failed: API connection error {}",
                    attempt, e
                );
            }
        }

        if attempt < cfg.max_checks {
            let exp_shift = (attempt as u64).min(3);
            let delay = cfg
                .initial_retry_interval_ms
                .saturating_mul(1 << exp_shift)
                .min(cfg.max_retry_interval_ms);
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }

    if last_error.is_empty() {
        last_error = "API not ready within stability window".to_string();
    }

    Err(last_error)
}

#[cfg(test)]
#[path = "readiness.tests.rs"]
mod tests;

