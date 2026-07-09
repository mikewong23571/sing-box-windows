use crate::app::constants::messages;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use reqwest::Client;
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, warn};

// 以管理员权限重启 (跨平台实现)
#[tauri::command]
pub fn restart_as_admin(app_handle: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        restart_as_admin_windows(app_handle)
    }

    #[cfg(target_os = "linux")]
    {
        restart_as_admin_linux(app_handle)
    }

    #[cfg(target_os = "macos")]
    {
        restart_as_admin_macos(app_handle)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err("当前平台不支持管理员权限提升".to_string())
    }
}

#[cfg(target_os = "windows")]
fn restart_as_admin_windows(app_handle: tauri::AppHandle) -> Result<(), String> {
    // 检查当前是否已经有管理员权限
    if check_admin() {
        return Ok(());
    }

    // 获取当前可执行文件路径
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("{}: {}", messages::ERR_GET_EXE_PATH_FAILED, e))?;
    let current_pid = std::process::id().to_string();

    // 确保文件存在
    if !current_exe.exists() {
        return Err(format!("找不到程序可执行文件: {}", current_exe.display()));
    }

    // 通过临时 PowerShell 脚本等待当前进程退出后，再以管理员权限启动新实例。
    // 这样可以避免：
    // 1. single-instance 在旧实例仍存活时把新管理员实例重定向回旧实例
    // 2. 依赖环境变量传参时，在提权边界上变量丢失导致的启动失败
    let escaped_exe_path = current_exe.to_string_lossy().replace('\'', "''");
    let relaunch_script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n\
         Wait-Process -Id {pid} -ErrorAction SilentlyContinue\n\
         Start-Sleep -Milliseconds 300\n\
         Start-Process -FilePath '{exe}'\n\
         Start-Sleep -Milliseconds 300\n\
         Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue\n",
        pid = current_pid,
        exe = escaped_exe_path
    );
    let script_path =
        std::env::temp_dir().join(format!("sing-box-windows-elevate-{}.ps1", current_pid));
    std::fs::write(&script_path, relaunch_script)
        .map_err(|e| format!("写入提权重启脚本失败: {}", e))?;
    let script_path_string = script_path.to_string_lossy().to_string();

    let result = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "Start-Process -FilePath powershell.exe -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-WindowStyle','Hidden','-File',$env:SINGBOX_ELEVATE_SCRIPT)",
        ])
        .env("SINGBOX_ELEVATE_SCRIPT", &script_path_string)
        .creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW)
        .spawn();

    match result {
        Ok(_) => {
            info!("已请求管理员权限重启应用，等待当前进程退出后重新拉起");
            // 启动成功，退出当前进程
            app_handle.exit(0);
            Ok(())
        }
        Err(e) => {
            warn!("使用 powershell 触发 UAC 失败: {}", e);

            // 备用方案：显式使用 powershell.exe 再尝试一次，处理 PATH 异常场景。
            let result = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    "Start-Process -FilePath powershell.exe -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-WindowStyle','Hidden','-File',$env:SINGBOX_ELEVATE_SCRIPT)",
                ])
                .env("SINGBOX_ELEVATE_SCRIPT", &script_path_string)
                .creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW)
                .spawn();

            match result {
                Ok(_) => {
                    info!("已通过备用命令请求管理员权限重启应用");
                    app_handle.exit(0);
                    Ok(())
                }
                Err(e2) => Err(format!(
                    "{}: 主方法失败: {}；备用方法失败: {}",
                    messages::ERR_RESTART_FAILED,
                    e,
                    e2
                )),
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn restart_as_admin_linux(app_handle: tauri::AppHandle) -> Result<(), String> {
    // 检查当前是否已经有root权限
    if check_admin() {
        return Ok(());
    }

    // 获取当前可执行文件路径
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("{}: {}", messages::ERR_GET_EXE_PATH_FAILED, e))?;

    // 确保文件存在
    if !current_exe.exists() {
        return Err(format!("找不到程序可执行文件: {}", current_exe.display()));
    }

    let exe_str = current_exe.to_string_lossy().to_string();

    // 优先使用 pkexec (Polkit)，这在大多数现代 Linux 发行版上都可用
    // 顺序：pkexec > gksu > kdesudo
    let sudo_commands = ["pkexec", "gksu", "kdesudo"];

    for sudo_cmd in sudo_commands {
        if which::which(sudo_cmd).is_ok() {
            // 构建命令，传递必要的显示环境变量（对于 Wayland/X11 兼容性）
            let mut cmd = std::process::Command::new(sudo_cmd);
            cmd.arg(&exe_str);

            // 传递显示相关环境变量，确保 GUI 应用能正常显示
            if let Ok(display) = std::env::var("DISPLAY") {
                cmd.env("DISPLAY", display);
            }
            if let Ok(wayland_display) = std::env::var("WAYLAND_DISPLAY") {
                cmd.env("WAYLAND_DISPLAY", wayland_display);
            }
            if let Ok(xdg_runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                cmd.env("XDG_RUNTIME_DIR", xdg_runtime_dir);
            }

            match cmd.spawn() {
                Ok(_) => {
                    // 启动成功，退出当前进程
                    app_handle.exit(0);
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("尝试使用 {} 提权失败: {}", sudo_cmd, e);
                    continue;
                }
            }
        }
    }

    // 最后尝试：使用终端运行 sudo
    if let Some(terminal) = find_terminal_emulator() {
        let sudo_cmd = format!("sudo '{}'", exe_str.replace("'", "'\\''"));
        let result = std::process::Command::new(&terminal)
            .arg("-e")
            .arg(&sudo_cmd)
            .spawn();

        if result.is_ok() {
            app_handle.exit(0);
            return Ok(());
        }
    }

    Err("未找到可用的权限提升工具 (pkexec, gksu, kdesudo) 或终端模拟器".to_string())
}

/// 查找可用的终端模拟器
#[cfg(target_os = "linux")]
pub(crate) fn find_terminal_emulator() -> Option<String> {
    let terminals = [
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "mate-terminal",
        "tilix",
        "terminator",
        "alacritty",
        "kitty",
        "xterm",
    ];
    for term in terminals {
        if which::which(term).is_ok() {
            return Some(term.to_string());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn restart_as_admin_macos(app_handle: tauri::AppHandle) -> Result<(), String> {
    // 检查当前是否已经有管理员权限
    if check_admin() {
        return Ok(());
    }

    // 获取当前可执行文件路径
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("{}: {}", messages::ERR_GET_EXE_PATH_FAILED, e))?;

    // 确保文件存在
    if !current_exe.exists() {
        return Err(format!("找不到程序可执行文件: {}", current_exe.display()));
    }

    let exe_path = current_exe.to_string_lossy();
    // 转义路径中的单引号，避免 AppleScript 语法错误
    let escaped_path = exe_path.replace("'", "'\\''");

    // 使用 osascript 通过 shell 以管理员权限运行应用
    // 注意：使用 open -n 来启动新实例，--args --elevated 可用于传递标志
    let apple_script = format!(
        r#"do shell script "open -n -a '{}' --args --elevated" with administrator privileges"#,
        escaped_path
    );

    tracing::info!("尝试使用 osascript 提权启动应用");

    let result = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&apple_script)
        .spawn();

    match result {
        Ok(_) => {
            // 延迟一下确保新进程启动
            std::thread::sleep(std::time::Duration::from_millis(500));
            // 启动成功，退出当前进程
            app_handle.exit(0);
            Ok(())
        }
        Err(e) => {
            tracing::warn!("osascript 提权失败: {}，尝试备用方法", e);

            // 备用方法：使用 Terminal 运行 sudo 命令
            let terminal_script = format!(
                r#"tell application "Terminal"
                    activate
                    do script "sudo '{}'"
                end tell"#,
                escaped_path
            );

            let result = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&terminal_script)
                .spawn();

            match result {
                Ok(_) => {
                    app_handle.exit(0);
                    Ok(())
                }
                Err(e2) => Err(format!(
                    "{}: {} 然后尝试备用方法失败: {}",
                    messages::ERR_RESTART_FAILED,
                    e,
                    e2
                )),
            }
        }
    }
}

// 检查是否有管理员权限 - 跨平台实现
#[tauri::command]
pub fn check_admin() -> bool {
    #[cfg(target_os = "windows")]
    {
        check_admin_windows()
    }

    #[cfg(target_os = "linux")]
    {
        check_admin_linux()
    }

    #[cfg(target_os = "macos")]
    {
        check_admin_macos()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        false // 其他平台默认返回false
    }
}

#[cfg(target_os = "windows")]
fn check_admin_windows() -> bool {
    // 尝试执行一个需要管理员权限的操作，例如查询系统会话
    let result = std::process::Command::new("net")
        .arg("session")
        .creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW)
        .output();

    match result {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn check_admin_linux() -> bool {
    // 直接使用 getuid() 检查是否为 root 用户
    // 这是最可靠的方式，不依赖可被伪造的环境变量
    nix::unistd::getuid().is_root()
}

#[cfg(target_os = "macos")]
fn check_admin_macos() -> bool {
    // TUN 需要 root 权限运行，检查当前进程的 effective UID
    // 注意：在 admin 组中不代表当前进程有 root 权限
    // 我们需要检查的是进程是否以 root 身份运行
    nix::unistd::geteuid().is_root()
}

// 打开开发者工具
#[tauri::command]
pub fn open_devtools(app_handle: tauri::AppHandle) -> Result<(), String> {
    let main_window = app_handle
        .get_webview_window("main")
        .ok_or("无法获取主窗口".to_string())?;

    main_window.open_devtools();
    Ok(())
}

const TCP_PROBE_TARGETS: [(&str, u16); 3] = [("1.1.1.1", 443), ("8.8.8.8", 53), ("223.5.5.5", 53)];

const HTTP_PROBE_URLS: [&str; 3] = [
    "https://connectivitycheck.gstatic.com/generate_204",
    "https://www.cloudflare.com/cdn-cgi/trace",
    "https://1.1.1.1",
];

/// 合并 TCP/HTTP 探测结果（纯逻辑）。
pub(crate) fn combine_connectivity_probe(
    tcp_success: bool,
    http_success: bool,
    strict_http: bool,
) -> bool {
    if strict_http {
        http_success
    } else {
        tcp_success || http_success
    }
}

/// 可注入目标的连通性探测（生产默认目标见 TCP_PROBE_TARGETS / HTTP_PROBE_URLS）。
pub(crate) async fn perform_network_probe_with(
    tcp_targets: &[(&str, u16)],
    http_urls: &[&str],
    strict_http: bool,
    tcp_timeout: Duration,
    http_timeout: Duration,
) -> Result<bool, String> {
    let mut tcp_success = false;

    for (host, port) in tcp_targets.iter() {
        let target = format!("{}:{}", host, port);
        match timeout(tcp_timeout, TcpStream::connect(&target)).await {
            Ok(Ok(_)) => {
                debug!("TCP 检测成功: {}", target);
                tcp_success = true;
                break;
            }
            Ok(Err(err)) => debug!("TCP 检测失败: {} -> {}", target, err),
            Err(_) => debug!("TCP 检测超时: {}", target),
        }
    }

    let client = Client::builder()
        .timeout(http_timeout)
        .user_agent("sing-box-windows/connectivity-check")
        .build()
        .map_err(|err| err.to_string())?;

    let mut http_success = false;
    for url in http_urls.iter() {
        match client.get(*url).send().await {
            Ok(response) => {
                if response.status().is_success() || response.status().as_u16() == 204 {
                    debug!("HTTP 检测成功: {}", url);
                    http_success = true;
                    break;
                } else {
                    debug!(
                        "HTTP 检测失败: {} -> 状态 {}",
                        url,
                        response.status().as_u16()
                    );
                }
            }
            Err(err) => debug!("HTTP 检测异常: {} -> {}", url, err),
        }
    }

    Ok(combine_connectivity_probe(
        tcp_success,
        http_success,
        strict_http,
    ))
}

async fn perform_network_probe(strict_http: bool) -> Result<bool, String> {
    perform_network_probe_with(
        &TCP_PROBE_TARGETS,
        &HTTP_PROBE_URLS,
        strict_http,
        Duration::from_secs(3),
        Duration::from_secs(6),
    )
    .await
}

#[tauri::command]
pub async fn check_network_connectivity(strict: Option<bool>) -> Result<bool, String> {
    perform_network_probe(strict.unwrap_or(false)).await
}

#[tauri::command]
pub async fn wait_for_network_ready(
    timeout_ms: Option<u64>,
    check_interval_ms: Option<u64>,
    strict: Option<bool>,
) -> Result<bool, String> {
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(60_000));
    let interval = Duration::from_millis(check_interval_ms.unwrap_or(3_000));
    let strict_mode = strict.unwrap_or(false);
    let start = Instant::now();
    let mut attempts = 0u32;

    loop {
        attempts += 1;
        match perform_network_probe(strict_mode).await {
            Ok(true) => {
                info!("网络在第{}次检查后已就绪", attempts);
                return Ok(true);
            }
            Ok(false) => {
                if start.elapsed() >= timeout {
                    warn!("网络检查超时，累计尝试 {} 次", attempts);
                    return Ok(false);
                }
            }
            Err(err) => {
                warn!("网络检查失败: {}", err);
                if start.elapsed() >= timeout {
                    return Err(err);
                }
            }
        }

        if start.elapsed() >= timeout {
            warn!("网络检查已达到超时时间");
            return Ok(false);
        }

        let remaining = timeout
            .checked_sub(start.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        sleep(std::cmp::min(interval, remaining)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_admin_returns_bool_without_panic() {
        // 普通用户环境下通常为 false；root 下为 true
        let _ = check_admin();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn find_terminal_emulator_is_optional() {
        // 仅验证不 panic；CI 可能无图形终端
        let _ = find_terminal_emulator();
    }

    #[tokio::test]
    async fn wait_for_network_ready_short_timeout_returns() {
        // 极短超时，确保函数可退出（网络可能已就绪也可能未就绪）
        let result = wait_for_network_ready(Some(50), Some(10), Some(false)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn check_network_connectivity_non_strict() {
        // 可能因环境无外网而失败/成功，但不能 panic
        let _ = check_network_connectivity(Some(false)).await;
    }

    #[test]
    fn combine_connectivity_probe_matrix() {
        assert!(combine_connectivity_probe(true, false, false));
        assert!(!combine_connectivity_probe(true, false, true));
        assert!(combine_connectivity_probe(false, true, true));
        assert!(!combine_connectivity_probe(false, false, false));
        assert!(combine_connectivity_probe(true, true, true));
    }

    #[tokio::test]
    async fn perform_network_probe_with_local_targets() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 256];
                let _ = s.read(&mut buf).await;
                let _ = s
                    .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            }
        });

        let http_url = format!("http://127.0.0.1:{}/generate_204", port);
        let ok = perform_network_probe_with(
            &[("127.0.0.1", port)],
            &[http_url.as_str()],
            false,
            Duration::from_millis(500),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert!(ok);

        let strict = perform_network_probe_with(
            &[("127.0.0.1", 1)],
            &[http_url.as_str()],
            true,
            Duration::from_millis(100),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert!(strict);

        let fail = perform_network_probe_with(
            &[("127.0.0.1", 1)],
            &["http://127.0.0.1:1/nope"],
            false,
            Duration::from_millis(50),
            Duration::from_millis(100),
        )
        .await
        .unwrap();
        assert!(!fail);
    }
}

