use super::helpers::backup_existing_config;
use super::parser::extract_nodes_from_subscription;
use crate::app::constants::messages;
use crate::app::singbox::config_generator;
use crate::app::singbox::settings_patch::apply_port_settings_only;
use crate::app::storage::state_model::AppConfig;
use base64::{engine::general_purpose, Engine as _};
use serde_json::Value;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{error, info};

/// 尝试把订阅内容当作 Base64 解码成 UTF-8 文本。
///
/// 说明：不少机场会把 Clash YAML / URI 列表做一次 Base64 封装，且可能包含换行。
pub(super) fn try_decode_base64_to_text(raw: &str) -> Option<String> {
    let mut s: String = raw.split_whitespace().collect();
    if s.is_empty() {
        return None;
    }

    // 补齐 padding，兼容省略 '=' 的情况
    let rem = s.len() % 4;
    if rem != 0 {
        s.push_str(&"=".repeat(4 - rem));
    }

    let bytes = general_purpose::STANDARD
        .decode(&s)
        .or_else(|_| general_purpose::URL_SAFE.decode(&s))
        .ok()?;
    String::from_utf8(bytes).ok()
}

pub(super) fn write_downloaded_subscription_config(
    response_text: &str,
    use_original_config: bool,
    app_config: &AppConfig,
    target_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let work_dir = crate::utils::app_util::get_work_dir_sync();
    let sing_box_dir = Path::new(&work_dir).join("sing-box");

    if !sing_box_dir.exists() {
        info!("正在创建Sing-Box目录: {:?}", sing_box_dir);
        if let Err(e) = std::fs::create_dir_all(&sing_box_dir) {
            let err_msg = format!("创建Sing-Box目录失败: {}", e);
            error!("{}", err_msg);
            return Err(err_msg.into());
        }
    }

    if use_original_config {
        info!("使用原始订阅内容，仅修改必要的端口和地址");
        process_original_config(response_text, app_config, target_path)?;
        return Ok(());
    }

    let mut extracted_nodes = extract_nodes_from_subscription(response_text)?;
    info!("从原始内容提取到 {} 个节点", extracted_nodes.len());

    if extracted_nodes.is_empty() {
        info!("未从原始内容提取到节点，尝试base64解码...");

        if let Some(decoded_text) = try_decode_base64_to_text(response_text) {
            info!("base64 解码成功，重新从解码内容提取节点...");
            extracted_nodes = extract_nodes_from_subscription(&decoded_text)?;
            info!("从 base64 解码内容提取到 {} 个节点", extracted_nodes.len());
        }
    }

    if extracted_nodes.is_empty() {
        info!("标准解码方法均未提取到节点，尝试移除前缀后再解码...");

        let stripped_text = response_text
            .trim()
            .replace("vmess://", "")
            .replace("ss://", "")
            .replace("trojan://", "")
            .replace("vless://", "")
            .replace("hysteria2://", "");

        if let Ok(decoded) = general_purpose::STANDARD.decode(&stripped_text) {
            if let Ok(decoded_text) = String::from_utf8(decoded) {
                extracted_nodes = extract_nodes_from_subscription(&decoded_text)?;
                info!(
                    "从移除前缀后解码内容提取到 {} 个节点",
                    extracted_nodes.len()
                );
            }
        }
    }

    if extracted_nodes.is_empty() {
        error!("无法从订阅内容提取节点信息，已尝试所有解码方式");
        return Err(
            "无法从订阅内容提取节点信息（支持 sing-box JSON / Clash YAML / URI 列表，且可 base64 封装），请检查订阅链接或内容格式"
                .into(),
        );
    }

    info!(
        "成功提取到 {} 个节点，准备应用到配置",
        extracted_nodes.len()
    );

    let dir = target_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(&work_dir).join("sing-box"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        error!("{}: {}", messages::ERR_CREATE_DIR_FAILED, e);
    }

    let config = config_generator::generate_config_with_nodes(app_config, &extracted_nodes)
        .map_err(|e| format!("生成配置失败: {}", e))?;

    write_config_file(target_path, &config, "配置")
}

pub(super) fn write_manual_subscription_config(
    content: &str,
    use_original_config: bool,
    app_config: &AppConfig,
    target_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if use_original_config {
        info!("使用原始配置内容，仅调整端口和地址");
        process_original_config(content, app_config, target_path)?;
        return Ok(());
    }

    let mut extracted_nodes = extract_nodes_from_subscription(content)?;
    info!("从手动内容提取到 {} 个节点", extracted_nodes.len());

    if extracted_nodes.is_empty() {
        if let Some(decoded_text) = try_decode_base64_to_text(content) {
            info!("手动内容 base64 解码成功，重新提取节点");
            extracted_nodes = extract_nodes_from_subscription(&decoded_text)?;
            info!("从解码内容提取到 {} 个节点", extracted_nodes.len());
        }
    }

    if extracted_nodes.is_empty() {
        return Err("无法从配置内容提取节点，请检查格式".into());
    }

    let config = config_generator::generate_config_with_nodes(app_config, &extracted_nodes)
        .map_err(|e| format!("生成配置失败: {}", e))?;

    write_config_file(target_path, &config, "手动配置")
}

fn process_original_config(
    content: &str,
    app_config: &AppConfig,
    target_path: &Path,
) -> Result<(), Box<dyn Error>> {
    info!("处理原始订阅配置，仅调整端口");

    let mut config: Value = serde_json::from_str(content)?;
    apply_port_settings_only(&mut config, app_config);

    write_config_file(target_path, &config, "原始订阅配置（修改端口后）")
}

fn write_config_file(
    target_path: &Path,
    config: &Value,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    info!("正在保存{}到: {:?}", label, target_path);

    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            info!("创建配置目录: {:?}", parent);
            if let Err(e) = std::fs::create_dir_all(parent) {
                let err_msg = format!("创建配置目录失败: {}", e);
                error!("{}", err_msg);
                return Err(err_msg.into());
            }
        }
    }

    let _backup = backup_existing_config(target_path);

    let config_str = serde_json::to_string_pretty(config)?;
    let mut file = File::create(target_path)?;
    file.write_all(config_str.as_bytes())?;

    info!("{}已成功保存", label);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{try_decode_base64_to_text, write_manual_subscription_config};
    use crate::app::storage::state_model::AppConfig;
    use base64::{engine::general_purpose, Engine as _};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("sing-box-windows-materializer-{label}-{unique}"))
            .join("config.json")
    }

    #[test]
    fn base64_decode_accepts_missing_padding_and_whitespace() {
        let raw = "trojan://password@example.com:443#demo";
        let encoded = general_purpose::STANDARD
            .encode(raw.as_bytes())
            .trim_end_matches('=')
            .to_string();
        let formatted = format!("{} \n", encoded);

        let decoded = try_decode_base64_to_text(&formatted).expect("decode should work");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn manual_uri_materialization_writes_config_without_runtime() {
        let target = temp_config_path("manual-uri");
        let content = "trojan://password@example.com:443?security=tls&sni=example.com#demo";

        write_manual_subscription_config(content, false, &AppConfig::default(), &target)
            .expect("manual config should materialize");

        let output = std::fs::read_to_string(&target).expect("config should be written");
        assert!(output.contains("\"outbounds\""));
        assert!(output.contains("example.com"));

        if let Some(parent) = target.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}
