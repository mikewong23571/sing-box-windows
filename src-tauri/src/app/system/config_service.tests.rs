use super::*;
use crate::app::singbox::common::ensure_kernel_log_output;
use serde_json::json;

fn outside_config_path(file_name: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!(r"C:\external\{}", file_name)
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("/tmp/{}", file_name)
    }
}

fn has_private_ip_rule(rules: &[Value]) -> bool {
    rules.iter().any(|rule| {
        rule.get("ip_cidr")
            .and_then(|v| v.as_array())
            .is_some_and(|cidrs| {
                PRIVATE_IP_CIDRS
                    .iter()
                    .all(|cidr| cidrs.iter().any(|value| value.as_str() == Some(*cidr)))
            })
    })
}

#[test]
fn sanitize_file_name_should_replace_invalid_characters_and_fallback() {
    assert_eq!(
        sanitize_file_name("my config?.json", "config.json"),
        "my-config-.json"
    );
    assert_eq!(sanitize_file_name(".", "config.json"), "config.json");
    assert_eq!(sanitize_file_name("..", "config.json"), "config.json");
}

#[test]
fn normalize_active_config_local_path_should_use_default_for_missing_path() {
    let (path, migrated) = normalize_active_config_local_path(None);

    assert_eq!(path, paths::get_config_dir().join("config.json"));
    assert!(!migrated);
}

#[test]
fn normalize_active_config_local_path_should_keep_local_absolute_path() {
    let local_path = paths::get_config_dir().join("configs").join("manual.json");
    let local_path_str = local_path.to_string_lossy().to_string();

    let (normalized, migrated) = normalize_active_config_local_path(Some(local_path_str.as_str()));

    assert_eq!(normalized, local_path);
    assert!(!migrated);
}

#[test]
fn normalize_active_config_local_path_should_rebase_external_absolute_path() {
    let (normalized, migrated) =
        normalize_active_config_local_path(Some(&outside_config_path("custom?.json")));

    assert_eq!(
        normalized,
        paths::get_config_dir().join("configs").join("custom-.json")
    );
    assert!(migrated);
}

#[test]
fn sanitize_geoip_private_rule_sets_should_rewrite_route_entries() {
    let mut config_obj = json!({
        "route": {
            "rule_set": [
                { "tag": RS_GEOIP_PRIVATE },
                { "tag": "keep-me" }
            ],
            "rules": [
                { "rule_set": RS_GEOIP_PRIVATE, "outbound": "proxy" },
                { "rule_set": [RS_GEOIP_PRIVATE, "keep-tag"], "outbound": "proxy" }
            ]
        }
    })
    .as_object()
    .cloned()
    .expect("config json should be an object");

    sanitize_geoip_private_rule_sets(&mut config_obj);

    let route = config_obj
        .get("route")
        .and_then(|value| value.as_object())
        .expect("route should remain an object");
    let rule_sets = route
        .get("rule_set")
        .and_then(|value| value.as_array())
        .expect("rule_set should remain an array");
    let rules = route
        .get("rules")
        .and_then(|value| value.as_array())
        .expect("rules should remain an array");

    assert_eq!(rule_sets.len(), 1);
    assert_eq!(rule_sets[0]["tag"].as_str(), Some("keep-me"));
    assert_eq!(rules[0]["rule_set"].as_str(), Some(RS_GEOSITE_PRIVATE));

    let second_rule_sets = rules[1]["rule_set"]
        .as_array()
        .expect("rule_set array should remain after filtering");
    assert_eq!(second_rule_sets.len(), 1);
    assert_eq!(second_rule_sets[0].as_str(), Some("keep-tag"));
    assert!(has_private_ip_rule(rules));
}

#[test]
fn ensure_kernel_log_output_should_insert_file_output() {
    let mut config_obj = json!({
        "route": {
            "rules": []
        }
    })
    .as_object()
    .cloned()
    .expect("config json should be an object");

    ensure_kernel_log_output(&mut config_obj);

    let log = config_obj
        .get("log")
        .and_then(|value| value.as_object())
        .expect("log object should be inserted");
    assert_eq!(log.get("disabled").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(log.get("level").and_then(|v| v.as_str()), Some("info"));
    assert_eq!(log.get("timestamp").and_then(|v| v.as_bool()), Some(true));

    let output = log
        .get("output")
        .and_then(|value| value.as_str())
        .expect("log.output should be inserted");
    assert!(
        output.ends_with("sing-box.log"),
        "log.output should point at sing-box.log: {output}"
    );
}

#[test]
fn ensure_kernel_log_output_should_replace_invalid_log_value() {
    let mut config_obj = json!({
        "log": null
    })
    .as_object()
    .cloned()
    .expect("config json should be an object");

    ensure_kernel_log_output(&mut config_obj);

    assert!(config_obj
        .get("log")
        .and_then(|value| value.get("output"))
        .and_then(|value| value.as_str())
        .is_some_and(|output| output.ends_with("sing-box.log")));
}

#[test]
fn ensure_private_ip_rule_should_not_duplicate_existing_rule() {
    let mut rules = vec![json!({
        "ip_cidr": PRIVATE_IP_CIDRS,
        "outbound": TAG_DIRECT
    })];

    ensure_private_ip_rule(&mut rules);

    assert_eq!(rules.len(), 1);
}

#[test]
fn ensure_private_ip_rule_should_append_rule_when_missing() {
    let mut rules = vec![json!({
        "domain_suffix": ["example.com"],
        "outbound": "proxy"
    })];

    ensure_private_ip_rule(&mut rules);

    assert_eq!(rules.len(), 2);
    assert!(has_private_ip_rule(&rules));
}

#[test]
fn write_default_and_restore_from_bak() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    write_default_config(&path, &AppConfig::default()).unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("inbounds") || content.contains("outbounds") || content.contains("log"));

    let bak = path.with_extension("bak");
    std::fs::copy(&path, &bak).unwrap();
    std::fs::write(&path, b"corrupted{{{").unwrap();
    assert!(try_restore_from_bak(&path).unwrap());
    let restored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(restored.is_object());

    // invalid bak
    std::fs::write(&bak, b"not-json").unwrap();
    std::fs::write(&path, b"bad").unwrap();
    assert!(!try_restore_from_bak(&path).unwrap());
    assert!(!try_restore_from_bak(dir.path().join("missing.json").as_path()).unwrap());
}

#[test]
fn backup_corrupted_config_renames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    std::fs::write(&path, b"{}").unwrap();
    backup_corrupted_config(&path);
    assert!(!path.exists());
    let backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!backups.is_empty());
    backup_corrupted_config(dir.path().join("nope.json").as_path()); // no-op
}

#[test]
fn ensure_private_ip_rule_idempotent() {
    let mut rules = vec![];
    ensure_private_ip_rule(&mut rules);
    assert_eq!(rules.len(), 1);
    ensure_private_ip_rule(&mut rules);
    assert_eq!(rules.len(), 1);
}

#[test]
fn normalize_relative_and_config_json_external() {
    let (p, migrated) = normalize_active_config_local_path(Some("configs/x.json"));
    assert!(p.to_string_lossy().contains("configs"));
    assert!(migrated);

    let (p2, m2) = normalize_active_config_local_path(Some(&outside_config_path("config.json")));
    assert!(p2.ends_with("config.json"));
    assert!(m2);
}

#[test]
fn patch_ports_into_config_json_all_branches() {
    use crate::app::singbox::config_generator::generate_base_config;
    use crate::app::storage::state_model::AppConfig;

    let mut cfg = serde_json::to_value(generate_base_config(&AppConfig::default())).unwrap();
    // ensure mixed-in exists or inject
    if cfg["inbounds"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        cfg["inbounds"] = json!([{"type":"mixed","tag":"mixed-in","listen_port":1}]);
    }
    patch_ports_into_config_json(&mut cfg, 18080, 19090).unwrap();
    assert!(cfg["experimental"]["clash_api"]["external_controller"]
        .as_str()
        .unwrap()
        .contains("19090"));

    // no experimental
    let mut bare = json!({"inbounds":[{"tag":"mixed-in","listen_port":1}]});
    patch_ports_into_config_json(&mut bare, 20000, 20001).unwrap();
    assert!(bare["experimental"]["clash_api"]["external_controller"]
        .as_str()
        .unwrap()
        .contains("20001"));
    assert_eq!(bare["inbounds"][0]["listen_port"], 20000);

    assert!(validate_proxy_api_ports(100, 2000).is_err());
    assert!(validate_proxy_api_ports(2000, 2000).is_err());
    assert!(validate_proxy_api_ports(2000, 2001).is_ok());
}
