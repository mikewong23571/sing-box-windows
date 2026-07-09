use crate::app::core::kernel_service::status::is_kernel_running;
use crate::utils::http_client;
use std::time::Duration;
use tracing::info;

pub(super) fn classify_startup_stability_failure(detail: &str) -> (&'static str, &'static str) {
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

pub(super) async fn verify_kernel_startup_stability(api_port: u16) -> Result<(), String> {
    const MAX_CHECKS: u8 = 10;
    const INITIAL_RETRY_INTERVAL_MS: u64 = 300;
    const MAX_RETRY_INTERVAL_MS: u64 = 2000;
    const API_TIMEOUT_MS: u64 = 1000;

    let client = http_client::get_client();
    let api_url = format!("http://127.0.0.1:{}/version", api_port);
    let mut last_error = String::new();

    for attempt in 1..=MAX_CHECKS {
        if !is_kernel_running().await.unwrap_or(false) {
            return Err("kernel process exited immediately after startup".to_string());
        }

        match client
            .get(&api_url)
            .timeout(Duration::from_millis(API_TIMEOUT_MS))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                info!(
                    "kernel stability check passed (attempt {}/{})",
                    attempt, MAX_CHECKS
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

        if attempt < MAX_CHECKS {
            let exp_shift = (attempt as u64).min(3);
            let delay = INITIAL_RETRY_INTERVAL_MS
                .saturating_mul(1 << exp_shift)
                .min(MAX_RETRY_INTERVAL_MS);
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }

    if last_error.is_empty() {
        last_error = "API not ready within stability window".to_string();
    }

    Err(last_error)
}
