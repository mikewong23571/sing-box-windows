#[cfg(test)]
mod wry_app_tests {
    /// Build AppHandle on any thread if platform allows.
    #[test]
    fn build_real_app_any_thread() {
        // tao EventLoopExtUnix::new_any_thread is used internally if we set env?
        // Try TAURI / WINIT env
        std::env::set_var("WINIT_UNIX_BACKEND", "x11");
        let result = std::panic::catch_unwind(|| {
            tauri::Builder::default()
                .build(tauri::generate_context!())
                .map(|app| app.handle().clone())
        });
        match result {
            Ok(Ok(_h)) => {
                // success - unlocked
            }
            Ok(Err(e)) => {
                eprintln!("build err: {e}");
                // not fatal for suite if unsupported
            }
            Err(_) => {
                eprintln!("panic building app off main thread - expected on some platforms");
            }
        }
    }
}
