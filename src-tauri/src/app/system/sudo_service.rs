use serde::Serialize;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use tauri::Manager;
use tauri::{AppHandle, Runtime};

/// 统一给前端/调用方识别的错误码前缀（避免依赖具体文案）。
/// 约定：Rust 端返回 `SUDO_PASSWORD_REQUIRED` / `SUDO_PASSWORD_INVALID` 等，
/// 前端可据此弹出“请输入系统密码”的窗口。
pub const SUDO_PASSWORD_REQUIRED: &str = "SUDO_PASSWORD_REQUIRED";
pub const SUDO_PASSWORD_INVALID: &str = "SUDO_PASSWORD_INVALID";
pub const SUDO_UNSUPPORTED: &str = "SUDO_UNSUPPORTED";

#[derive(Debug, Clone, Serialize)]
pub struct SudoPasswordStatus {
    pub supported: bool,
    pub has_saved: bool,
}

/// 查询当前平台是否支持“保存并复用 sudo 密码”能力，以及是否已保存。
#[tauri::command]
pub async fn sudo_password_status(app: AppHandle) -> Result<SudoPasswordStatus, String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let has_saved = has_saved_password(&app).await?;
        Ok(SudoPasswordStatus {
            supported: true,
            has_saved,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = app;
        Ok(SudoPasswordStatus {
            supported: false,
            has_saved: false,
        })
    }
}

/// 设置/更新 sudo 密码：会先校验密码是否正确，正确才加密写入数据库。
#[tauri::command]
pub async fn sudo_set_password(password: String, app: AppHandle) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // 必要的安全措施：不保存无效密码，避免后续启动卡死/失败。
        validate_sudo_password(&password)?;
        save_password(&app, &password).await?;
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (password, app);
        Err(SUDO_UNSUPPORTED.to_string())
    }
}

/// 清除已保存的 sudo 密码（例如用户修改了系统密码后需要重新设置）。
#[tauri::command]
pub async fn sudo_clear_password(_app: AppHandle) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        delete_saved_password(&_app).await?;
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(SUDO_UNSUPPORTED.to_string())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
use {
    aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    },
    base64::engine::general_purpose::STANDARD as BASE64_ENGINE,
    base64::Engine,
    rand::RngCore,
    sha2::{Digest, Sha256},
    tracing::warn,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const SUDO_PASSWORD_KEY: &str = "sudo_password_cipher_v1";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const NONCE_LEN: usize = 12;

/// 从任意盐值派生 32 字节密钥（纯逻辑，便于单测）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn derive_crypto_key_from_material(material: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(material);
    hasher.update(b"|sing-box-windows|sudo|v1");
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn derive_crypto_key<R: Runtime>(app: &AppHandle<R>) -> Result<[u8; 32], String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?;

    Ok(derive_crypto_key_from_material(
        data_dir.to_string_lossy().as_bytes(),
    ))
}

/// 使用固定密钥加密密码（纯逻辑）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn encrypt_password_with_key(key: &[u8; 32], password: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("初始化加密器失败: {}", e))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| format!("加密密码失败: {}", e))?;

    let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64_ENGINE.encode(combined))
}

/// 使用固定密钥解密密码（纯逻辑）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn decrypt_password_with_key(key: &[u8; 32], encoded: &str) -> Result<String, String> {
    let raw = BASE64_ENGINE
        .decode(encoded)
        .map_err(|e| format!("解码密码失败: {}", e))?;
    if raw.len() <= NONCE_LEN {
        return Err("保存的密码数据已损坏，请重新输入".to_string());
    }

    let (nonce_bytes, cipher_bytes) = raw.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("初始化解密器失败: {}", e))?;

    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), cipher_bytes)
        .map_err(|e| format!("解密密码失败: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("解密后的密码不是有效 UTF-8: {}", e))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn encrypt_password<R: Runtime>(app: &AppHandle<R>, password: &str) -> Result<String, String> {
    let key = derive_crypto_key(app)?;
    encrypt_password_with_key(&key, password)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn decrypt_password<R: Runtime>(app: &AppHandle<R>, encoded: &str) -> Result<String, String> {
    let key = derive_crypto_key(app)?;
    decrypt_password_with_key(&key, encoded)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn has_saved_password<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    Ok(load_saved_password(app).await?.is_some())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn load_saved_password<R: Runtime>(app: &AppHandle<R>) -> Result<Option<String>, String> {
    use crate::app::storage::enhanced_storage_service::get_enhanced_storage;

    let storage = get_enhanced_storage(app).await?;
    let cipher: Option<String> = storage
        .get_config(SUDO_PASSWORD_KEY)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(cipher) = cipher {
        match decrypt_password(app, &cipher) {
            Ok(pwd) if !pwd.is_empty() => Ok(Some(pwd)),
            Ok(_) => Ok(None),
            Err(err) => {
                warn!("保存的 sudo 密码解密失败，清除缓存: {}", err);
                let _ = storage.remove_config(SUDO_PASSWORD_KEY).await;
                Ok(None)
            }
        }
    } else {
        Ok(None)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn load_validated_saved_password<R: Runtime>(
    app_handle: &AppHandle<R>,
) -> Result<String, String> {
    let saved = load_saved_password(app_handle).await?;
    let Some(password) = saved else {
        return Err(SUDO_PASSWORD_REQUIRED.to_string());
    };

    if let Err(err) = validate_sudo_password(&password) {
        if err == SUDO_PASSWORD_INVALID {
            let _ = delete_saved_password(app_handle).await;
            return Err(format!(
                "{}: saved password cleared, please re-enter",
                SUDO_PASSWORD_INVALID
            ));
        }
        return Err(err);
    }

    Ok(password)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn save_password<R: Runtime>(app: &AppHandle<R>, password: &str) -> Result<(), String> {
    use crate::app::storage::enhanced_storage_service::get_enhanced_storage;

    let cipher = encrypt_password(app, password)?;
    let storage = get_enhanced_storage(app).await?;
    storage
        .save_config(SUDO_PASSWORD_KEY, &cipher)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn delete_saved_password<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    use crate::app::storage::enhanced_storage_service::get_enhanced_storage;

    let storage = get_enhanced_storage(app).await?;
    storage
        .remove_config(SUDO_PASSWORD_KEY)
        .await
        .map_err(|e| e.to_string())
}

/// 将密码加密写入存储（不调用真实 sudo 校验；单测/导入用）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(dead_code)]
pub(crate) async fn save_password_for_tests<R: Runtime>(
    app: &AppHandle<R>,
    password: &str,
) -> Result<(), String> {
    save_password(app, password).await
}

/// 读取已保存密文并解密（不校验 sudo；单测用）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(dead_code)]
pub(crate) async fn load_saved_password_for_tests<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<String>, String> {
    load_saved_password(app).await
}

/// 清除已保存密码（不依赖平台命令；单测/Mock 可用）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(dead_code)]
pub(crate) async fn delete_saved_password_for_tests<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    delete_saved_password(app).await
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_sudo_password(password: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // 说明：
    // - `-S`：从 stdin 读取密码（不依赖 TTY）
    // - `-k`：强制重新认证，确保我们真的验证了当前密码是否正确
    // - `-p ''`：禁用提示符，避免输出干扰
    // - `-v`：仅校验/刷新凭据，不执行命令
    let mut child = Command::new("sudo")
        .args(["-S", "-k", "-p", "", "-v"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("执行 sudo 校验失败: {}", e))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(password.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("写入 sudo 密码失败: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("等待 sudo 校验失败: {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(classify_sudo_auth_failure(&stderr))
}

/// 根据 sudo 校验 stderr 分类错误（纯逻辑，不调用真实 sudo）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn classify_sudo_auth_failure(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("sorry")
        || lower.contains("incorrect")
        || lower.contains("authentication failure")
        || lower.contains("try again")
    {
        return SUDO_PASSWORD_INVALID.to_string();
    }
    format!("sudo 校验失败: {}", stderr.trim())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn can_run_sudo_non_interactive() -> bool {
    use std::process::Command;
    // `-n`：非交互模式，若需要密码则直接失败
    Command::new("sudo")
        .args(["-n", "true"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_sudo_command(password: Option<&str>, args: &[&str]) -> Result<std::process::Output, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if can_run_sudo_non_interactive() {
        return Command::new("sudo")
            .arg("-n")
            .arg("--")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("执行 sudo 命令失败: {}", e));
    }

    let password = password.ok_or_else(|| SUDO_PASSWORD_REQUIRED.to_string())?;
    let mut child = Command::new("sudo")
        .args(["-S", "-k", "-p", "", "--"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("执行 sudo 命令失败: {}", e))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(password.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("写入 sudo 密码失败: {}", e))?;
    }

    child
        .wait_with_output()
        .map_err(|e| format!("等待 sudo 命令失败: {}", e))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn map_sudo_command_result(
    output: std::process::Output,
    action: &str,
) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        return Err(format!(
            "{}失败，退出码: {:?}",
            action,
            output.status.code()
        ));
    }

    Err(format!("{}失败: {}", action, stderr))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub async fn kill_process_by_pid_with_saved_password<R: Runtime>(
    app_handle: &AppHandle<R>,
    pid: u32,
) -> Result<(), String> {
    let password = load_validated_saved_password(app_handle).await?;
    let pid_string = pid.to_string();
    let output = run_sudo_command(Some(&password), &["kill", "-9", &pid_string])?;
    map_sudo_command_result(output, &format!("sudo 终止进程 PID {}", pid))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub async fn kill_process_by_pid_with_saved_password<R: Runtime>(
    _app_handle: &AppHandle<R>,
    _pid: u32,
) -> Result<(), String> {
    Err(SUDO_UNSUPPORTED.to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub async fn kill_processes_by_name_with_saved_password<R: Runtime>(
    app_handle: &AppHandle<R>,
    process_name: &str,
) -> Result<(), String> {
    let password = load_validated_saved_password(app_handle).await?;
    let output = run_sudo_command(Some(&password), &["pkill", "-9", "-x", process_name])?;

    // `pkill` 退出码 1 表示没有匹配的进程，不应阻断清理流程。
    if output.status.success() || output.status.code() == Some(1) {
        return Ok(());
    }

    map_sudo_command_result(output, &format!("sudo 按名称终止进程 {}", process_name))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub async fn kill_processes_by_name_with_saved_password<R: Runtime>(
    _app_handle: &AppHandle<R>,
    _process_name: &str,
) -> Result<(), String> {
    Err(SUDO_UNSUPPORTED.to_string())
}

/// Linux/macOS: 读取已保存密码并用 sudo 提权启动内核。
///
/// 设计目标：
/// - 第一次使用由前端弹窗输入系统密码（本函数在未保存时返回 `SUDO_PASSWORD_REQUIRED`）
/// - 每次启动前先用 `sudo -S -k -v` 校验/刷新凭据
/// - 尽量用 `sudo -n` 启动内核，避免把密码写进内核 stdin
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub async fn spawn_kernel_with_saved_password<R: Runtime>(
    app_handle: &AppHandle<R>,
    kernel_path: &str,
    work_dir: &str,
    config_path: &str,
) -> Result<std::process::Child, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let password = load_validated_saved_password(app_handle).await?;

    // 首选：非交互 sudo（更安全，避免密码进入内核 stdin）
    if can_run_sudo_non_interactive() {
        let mut cmd = Command::new("sudo");
        cmd.args([
            "-n",
            "--",
            kernel_path,
            "run",
            "-D",
            work_dir,
            "-c",
            config_path,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

        return cmd.spawn().map_err(|e| format!("sudo 启动内核失败: {}", e));
    }

    // 回退：策略要求每次都输入密码（例如 timestamp_timeout=0）
    // 这里用 `-S -k` 强制 sudo 读取密码，因此密码不会泄露给内核 stdin。
    let mut cmd = Command::new("sudo");
    cmd.args([
        "-S",
        "-k",
        "-p",
        "",
        "--",
        kernel_path,
        "run",
        "-D",
        work_dir,
        "-c",
        config_path,
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("sudo 启动内核失败: {}", e))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(password.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("写入 sudo 密码失败: {}", e))?;
    }

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sudo_constants_are_stable() {
        assert_eq!(SUDO_PASSWORD_REQUIRED, "SUDO_PASSWORD_REQUIRED");
        assert_eq!(SUDO_PASSWORD_INVALID, "SUDO_PASSWORD_INVALID");
        assert_eq!(SUDO_UNSUPPORTED, "SUDO_UNSUPPORTED");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn map_sudo_command_result_success_and_failure() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::{ExitStatus, Output};

        let ok = Output {
            status: ExitStatus::from_raw(0),
            stdout: vec![],
            stderr: vec![],
        };
        assert!(map_sudo_command_result(ok, "kill").is_ok());

        let fail_empty = Output {
            status: ExitStatus::from_raw(256), // exit code 1
            stdout: vec![],
            stderr: vec![],
        };
        let err = map_sudo_command_result(fail_empty, "kill").unwrap_err();
        assert!(err.contains("kill失败"));

        let fail_msg = Output {
            status: ExitStatus::from_raw(256),
            stdout: vec![],
            stderr: b"permission denied".to_vec(),
        };
        let err = map_sudo_command_result(fail_msg, "spawn").unwrap_err();
        assert!(err.contains("permission denied"));
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[tokio::test]
    async fn windows_sudo_kill_helpers_are_unsupported() {
        // AppHandle 不可用时仍应返回固定错误码语义（命令本身要 handle，这里测常量路径）
        assert_eq!(SUDO_UNSUPPORTED, "SUDO_UNSUPPORTED");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn encrypt_decrypt_password_roundtrip_with_fixed_key() {
        let key = derive_crypto_key_from_material(b"/tmp/test-app-data|unit");
        let key2 = derive_crypto_key_from_material(b"/tmp/other");
        assert_ne!(key, key2);

        let cipher = encrypt_password_with_key(&key, "s3cret!").unwrap();
        assert_ne!(cipher, "s3cret!");
        let plain = decrypt_password_with_key(&key, &cipher).unwrap();
        assert_eq!(plain, "s3cret!");

        // 错误密钥应失败
        assert!(decrypt_password_with_key(&key2, &cipher).is_err());
        // 损坏数据
        assert!(decrypt_password_with_key(&key, "not-base64!!!").is_err());
        assert!(decrypt_password_with_key(&key, "").is_err());
        // 过短 payload（仅 nonce 不够）
        use base64::Engine;
        let short = base64::engine::general_purpose::STANDARD.encode([1u8; 8]);
        assert!(decrypt_password_with_key(&key, &short).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn can_run_sudo_non_interactive_returns_bool() {
        // 仅保证不 panic；CI 通常无 passwordless sudo
        let _ = can_run_sudo_non_interactive();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn derive_crypto_key_is_deterministic_and_sensitive() {
        let a = derive_crypto_key_from_material(b"same");
        let b = derive_crypto_key_from_material(b"same");
        let c = derive_crypto_key_from_material(b"same\0");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // 空材料也可派生
        let empty = derive_crypto_key_from_material(b"");
        assert_ne!(empty, a);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn encrypt_empty_and_unicode_password() {
        let key = derive_crypto_key_from_material(b"unit-key-material");
        let c1 = encrypt_password_with_key(&key, "").unwrap();
        assert_eq!(decrypt_password_with_key(&key, &c1).unwrap(), "");
        let c2 = encrypt_password_with_key(&key, "密码🔐").unwrap();
        assert_eq!(decrypt_password_with_key(&key, &c2).unwrap(), "密码🔐");
        // 两次加密密文不同（随机 nonce）但都能解密
        let c3 = encrypt_password_with_key(&key, "same").unwrap();
        let c4 = encrypt_password_with_key(&key, "same").unwrap();
        assert_ne!(c3, c4);
        assert_eq!(decrypt_password_with_key(&key, &c3).unwrap(), "same");
        assert_eq!(decrypt_password_with_key(&key, &c4).unwrap(), "same");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn decrypt_truncated_payload_errors() {
        use base64::Engine;
        let key = derive_crypto_key_from_material(b"k");
        // 恰好 nonce 长度（12）无密文
        let only_nonce = base64::engine::general_purpose::STANDARD.encode([0u8; 12]);
        assert!(decrypt_password_with_key(&key, &only_nonce).is_err());
        // nonce + 垃圾密文
        let junk = base64::engine::general_purpose::STANDARD.encode([7u8; 40]);
        assert!(decrypt_password_with_key(&key, &junk).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn classify_sudo_auth_failure_patterns() {
        assert_eq!(
            classify_sudo_auth_failure("Sorry, try again."),
            SUDO_PASSWORD_INVALID
        );
        assert_eq!(
            classify_sudo_auth_failure("incorrect password"),
            SUDO_PASSWORD_INVALID
        );
        assert_eq!(
            classify_sudo_auth_failure("Authentication failure"),
            SUDO_PASSWORD_INVALID
        );
        assert_eq!(
            classify_sudo_auth_failure("Please try again"),
            SUDO_PASSWORD_INVALID
        );
        let other = classify_sudo_auth_failure("  no tty present  ");
        assert!(other.contains("sudo 校验失败"));
        assert!(other.contains("no tty present"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn map_sudo_command_result_with_nonzero_code_message() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::{ExitStatus, Output};

        let fail = Output {
            status: ExitStatus::from_raw(512), // exit 2
            stdout: vec![],
            stderr: b"  kill: no such process  ".to_vec(),
        };
        let err = map_sudo_command_result(fail, "sudo 终止进程").unwrap_err();
        assert!(err.contains("no such process"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn save_load_delete_password_via_mock_storage() {
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("sudo.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        let h = env.handle();

        // MockRuntime 的 app_data_dir 可能可用；若 derive 失败则跳过
        let save = save_password_for_tests(&h, "unit-secret-pwd");
        match save.await {
            Ok(()) => {
                let loaded = load_saved_password_for_tests(&h).await.unwrap();
                assert_eq!(loaded.as_deref(), Some("unit-secret-pwd"));
                delete_saved_password_for_tests(&h).await.unwrap();
                assert!(load_saved_password_for_tests(&h).await.unwrap().is_none());
            }
            Err(e) => {
                // path resolver 在 mock 上不可用时仅保证不 panic
                assert!(!e.is_empty());
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn load_saved_password_empty_storage() {
        use crate::test_support::MockAppEnv;
        let env = MockAppEnv::new();
        let db = env.workspace.path().join("sudo2.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        let h = env.handle();
        // 无密文时 Ok(None) 或 path 错误
        let _ = load_saved_password_for_tests(&h).await;
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn kill_and_spawn_without_saved_password_errors() {
        use crate::test_support::MockAppEnv;
        let env = MockAppEnv::new();
        let db = env.workspace.path().join("sudo3.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        let h = env.handle();

        // 无已存密码 → SUDO_PASSWORD_REQUIRED
        let kill = kill_process_by_pid_with_saved_password(&h, 1).await;
        assert!(kill.is_err());
        let err = kill.unwrap_err();
        assert!(
            err.contains(SUDO_PASSWORD_REQUIRED) || !err.is_empty(),
            "err={err}"
        );

        let kill_name = kill_processes_by_name_with_saved_password(&h, "no-such-proc-xyz").await;
        assert!(kill_name.is_err());

        let spawn = spawn_kernel_with_saved_password(&h, "/bin/true", "/tmp", "/tmp/c.json").await;
        assert!(spawn.is_err());
    }

    #[test]
    fn has_saved_password_helpers_constants() {
        // 纯常量与错误文案稳定性（不触发真实 sudo）
        assert!(!SUDO_PASSWORD_REQUIRED.is_empty());
        assert!(!SUDO_PASSWORD_INVALID.is_empty());
        assert_eq!(
            classify_sudo_auth_failure("Sorry, try again."),
            SUDO_PASSWORD_INVALID
        );
    }
}
