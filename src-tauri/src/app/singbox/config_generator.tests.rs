use super::*;
use crate::app::core::tun_profile::default_tun_route_exclude_addresses;
use crate::app::storage::state_model::AppConfig;
use serde_json::Value;

fn assert_inbounds_do_not_contain_legacy_fields(config: &Value) {
    let inbounds = config
        .get("inbounds")
        .and_then(|v| v.as_array())
        .expect("inbounds 应存在");

    for inbound in inbounds {
        for legacy_field in [
            "sniff",
            "sniff_override_destination",
            "sniff_timeout",
            "domain_strategy",
            "udp_disable_domain_unmapping",
        ] {
            assert!(
                inbound.get(legacy_field).is_none(),
                "inbound 不应包含 legacy 字段 {}: {:?}",
                legacy_field,
                inbound
            );
        }
    }
}

fn assert_route_rules_keep_sniff_action(config: &Value) {
    let rules = config
        .get("route")
        .and_then(|v| v.get("rules"))
        .and_then(|v| v.as_array())
        .expect("route.rules 应存在");

    assert!(
        rules
            .iter()
            .any(|rule| rule.get("action").and_then(|v| v.as_str()) == Some("sniff")),
        "route.rules 应保留 sniff action: {:?}",
        rules
    );
}

#[test]
fn generated_dns_servers_should_use_new_format() {
    let config = generate_base_config(&AppConfig::default());
    let servers = config
        .get("dns")
        .and_then(|v| v.get("servers"))
        .and_then(|v| v.as_array())
        .expect("dns.servers 应存在");

    for server in servers {
        assert!(
            server.get("type").and_then(|v| v.as_str()).is_some(),
            "dns server 应包含 type 字段: {:?}",
            server
        );
        assert!(
            server.get("address").is_none(),
            "dns server 不应再输出 legacy address 字段: {:?}",
            server
        );
        assert!(
            server.get("address_resolver").is_none(),
            "dns server 不应再输出 legacy address_resolver 字段: {:?}",
            server
        );
        assert!(
            server.get("strategy").is_none(),
            "dns server 不应包含 strategy 字段（该字段属于 dns 根配置而非 server）: {:?}",
            server
        );
        assert!(
            server.get("domain_strategy").is_none(),
            "dns server 不应包含已弃用的 domain_strategy 字段: {:?}",
            server
        );
        assert!(
            server.get("detour").and_then(|v| v.as_str()) != Some("direct"),
            "dns server 不应显式设置 detour=direct: {:?}",
            server
        );
    }

    let route_default_resolver = config
        .get("route")
        .and_then(|v| v.get("default_domain_resolver"))
        .expect("route.default_domain_resolver 应存在");
    assert_eq!(
        route_default_resolver
            .get("server")
            .and_then(|v| v.as_str()),
        Some(DNS_RESOLVER)
    );
    assert!(route_default_resolver.get("strategy").is_some());
}

#[test]
fn generated_log_should_write_to_kernel_work_dir_file() {
    let config = generate_base_config(&AppConfig::default());
    let log = config
        .get("log")
        .and_then(|v| v.as_object())
        .expect("log 配置应存在");

    assert_eq!(log.get("disabled").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(log.get("level").and_then(|v| v.as_str()), Some("info"));
    assert_eq!(log.get("timestamp").and_then(|v| v.as_bool()), Some(true));

    let output = log
        .get("output")
        .and_then(|v| v.as_str())
        .expect("log.output 应存在");
    assert!(
        output.ends_with("sing-box.log"),
        "log.output 应指向 sing-box.log: {output}"
    );
}

#[test]
fn ads_dns_rule_should_use_reject_action() {
    let app_config = AppConfig {
        singbox_block_ads: true,
        ..AppConfig::default()
    };

    let config = generate_base_config(&app_config);
    let rules = config
        .get("dns")
        .and_then(|v| v.get("rules"))
        .and_then(|v| v.as_array())
        .expect("dns.rules 应存在");

    let ads_rule = rules
        .iter()
        .find(|rule| rule.get("rule_set").and_then(|v| v.as_str()) == Some(RS_GEOSITE_ADS))
        .expect("启用广告拦截时应包含 geosite ads DNS 规则");

    assert_eq!(
        ads_rule.get("action").and_then(|v| v.as_str()),
        Some("reject")
    );
    assert!(ads_rule.get("server").is_none());
}

#[test]
fn fake_dns_should_append_fakeip_server_and_enable_reverse_mapping() {
    let app_config = AppConfig {
        singbox_fake_dns_enabled: true,
        ..AppConfig::default()
    };

    let config = generate_base_config(&app_config);
    let servers = config
        .get("dns")
        .and_then(|v| v.get("servers"))
        .and_then(|v| v.as_array())
        .expect("dns.servers 应存在");

    let fakeip_server = servers
        .iter()
        .find(|server| server.get("tag").and_then(|v| v.as_str()) == Some(DNS_FAKEIP))
        .expect("启用 fake dns 后应包含 fakeip dns server");

    assert_eq!(
        fakeip_server.get("type").and_then(|v| v.as_str()),
        Some("fakeip")
    );
    assert_eq!(
        fakeip_server.get("inet4_range").and_then(|v| v.as_str()),
        Some("198.18.0.0/15")
    );
    assert_eq!(
        fakeip_server.get("inet6_range").and_then(|v| v.as_str()),
        Some("fc00::/18")
    );

    assert_eq!(
        config
            .get("dns")
            .and_then(|v| v.get("reverse_mapping"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        config
            .get("experimental")
            .and_then(|v| v.get("cache_file"))
            .and_then(|v| v.get("store_rdrc"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn fake_dns_global_non_cn_should_add_catch_all_query_rule() {
    let app_config = AppConfig {
        singbox_fake_dns_enabled: true,
        singbox_fake_dns_filter_mode: "global_non_cn".to_string(),
        ..AppConfig::default()
    };

    let config = generate_base_config(&app_config);
    let rules = config
        .get("dns")
        .and_then(|v| v.get("rules"))
        .and_then(|v| v.as_array())
        .expect("dns.rules 应存在");

    let catch_all = rules.iter().find(|rule| {
        rule.get("server").and_then(|v| v.as_str()) == Some(DNS_FAKEIP)
            && rule.get("rule_set").is_none()
            && rule.get("query_type").is_some()
    });
    assert!(
        catch_all.is_some(),
        "global_non_cn 模式应生成 A/AAAA catch-all fakeip 规则"
    );
}

#[test]
fn generated_inbounds_should_not_use_legacy_fields() {
    let config = generate_base_config(&AppConfig::default());

    assert_inbounds_do_not_contain_legacy_fields(&config);
    assert_route_rules_keep_sniff_action(&config);
}

#[test]
fn generated_tun_inbounds_should_not_use_legacy_fields() {
    let app_config = AppConfig {
        tun_enabled: true,
        tun_enable_ipv6: true,
        ..AppConfig::default()
    };

    let config = generate_base_config(&app_config);
    let inbounds = config
        .get("inbounds")
        .and_then(|v| v.as_array())
        .expect("inbounds 应存在");

    assert_eq!(inbounds.len(), 2, "启用 TUN 时应生成 mixed + tun 两个入站");
    assert_inbounds_do_not_contain_legacy_fields(&config);
    assert_route_rules_keep_sniff_action(&config);
}

#[test]
fn generated_tun_inbound_should_use_canonical_route_exclude_address_default() {
    let app_config = AppConfig {
        tun_enabled: true,
        ..AppConfig::default()
    };

    let config = generate_base_config(&app_config);
    let tun_in = config
        .get("inbounds")
        .and_then(|value| value.as_array())
        .and_then(|inbounds| {
            inbounds.iter().find(|inbound| {
                inbound.get("tag").and_then(|value| value.as_str()) == Some("tun-in")
            })
        })
        .expect("tun-in 应存在");

    assert_eq!(
        tun_in.get("route_exclude_address"),
        Some(&serde_json::json!(default_tun_route_exclude_addresses()))
    );
}

#[test]
fn generated_tun_inbound_should_use_explicit_route_exclude_address_override() {
    let app_config = AppConfig {
        tun_enabled: true,
        tun_route_exclude_address: Some(vec!["203.0.113.0/24".to_string()]),
        ..AppConfig::default()
    };

    let config = generate_base_config(&app_config);
    let tun_in = config
        .get("inbounds")
        .and_then(|value| value.as_array())
        .and_then(|inbounds| {
            inbounds.iter().find(|inbound| {
                inbound.get("tag").and_then(|value| value.as_str()) == Some("tun-in")
            })
        })
        .expect("tun-in 应存在");

    assert_eq!(
        tun_in.get("route_exclude_address"),
        Some(&serde_json::json!(["203.0.113.0/24"]))
    );
}

// -------------------- 自定义规则 inject / strip 幂等 --------------------

fn sample_custom_rule(
    match_type: crate::app::storage::custom_rule::CustomRuleMatchType,
    payload: &str,
    action: crate::app::storage::custom_rule::CustomRuleAction,
) -> crate::app::storage::custom_rule::CustomRule {
    use chrono::Utc;
    crate::app::storage::custom_rule::CustomRule {
        id: format!("id-{}", payload),
        enabled: true,
        match_type,
        payload: payload.to_string(),
        action,
        outbound: None,
        note: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn route_rules(config: &Value) -> &[Value] {
    config
        .get("route")
        .and_then(|r| r.get("rules"))
        .and_then(|r| r.as_array())
        .expect("route.rules 应存在")
}

#[test]
fn inject_custom_rules_marks_each_rule_and_preserves_preamble() {
    use crate::app::storage::custom_rule::{CustomRuleAction, CustomRuleMatchType};

    let mut config = generate_base_config(&AppConfig::default());
    let before_len = route_rules(&config).len();
    assert!(before_len > 0, "默认配置应有系统 route.rules");

    let rules = vec![
        sample_custom_rule(
            CustomRuleMatchType::DomainSuffix,
            "example.com",
            CustomRuleAction::Direct,
        ),
        sample_custom_rule(
            CustomRuleMatchType::IpCidr,
            "10.0.0.0/8",
            CustomRuleAction::Block,
        ),
    ];
    let injected = inject_custom_rules(&mut config, &rules, "手动选择");
    assert_eq!(injected, 2);

    let rules_arr = route_rules(&config);
    assert_eq!(rules_arr.len(), before_len + 2);

    // sniff 等系统规则仍在，且不应被打上自定义标记
    assert!(
        rules_arr
            .iter()
            .any(|r| r.get("action").and_then(|a| a.as_str()) == Some("sniff")),
        "sniff 系统规则应保留"
    );

    let marked: Vec<_> = rules_arr
        .iter()
        .filter(|r| r.get(CUSTOM_RULE_MARKER).is_some())
        .collect();
    assert_eq!(marked.len(), 2, "每条自定义规则都应有 marker");
    assert!(
        marked
            .iter()
            .all(|r| { r.get("domain_suffix").is_some() || r.get("ip_cidr").is_some() }),
        "marker 只应出现在自定义规则上"
    );
}

#[test]
fn strip_custom_rules_removes_all_custom_and_keeps_system() {
    use crate::app::storage::custom_rule::{CustomRuleAction, CustomRuleMatchType};

    let mut config = generate_base_config(&AppConfig::default());
    let baseline = route_rules(&config).to_vec();

    let rules = vec![
        sample_custom_rule(
            CustomRuleMatchType::Domain,
            "a.com",
            CustomRuleAction::Proxy,
        ),
        sample_custom_rule(
            CustomRuleMatchType::DomainSuffix,
            "b.com",
            CustomRuleAction::Direct,
        ),
        sample_custom_rule(
            CustomRuleMatchType::IpCidr,
            "192.168.0.0/16",
            CustomRuleAction::Block,
        ),
    ];
    inject_custom_rules(&mut config, &rules, "自动选择");
    assert_eq!(route_rules(&config).len(), baseline.len() + 3);

    strip_custom_rules(&mut config);
    let after = route_rules(&config);
    assert_eq!(after.len(), baseline.len(), "strip 后长度应回到基线");
    assert!(
        after.iter().all(|r| r.get(CUSTOM_RULE_MARKER).is_none()),
        "strip 后不应残留 marker"
    );
    // 系统规则内容仍在
    assert!(
        after
            .iter()
            .any(|r| r.get("action").and_then(|a| a.as_str()) == Some("sniff")),
        "strip 不得删除 sniff 等系统规则"
    );
}

#[test]
fn inject_strip_cycle_is_idempotent_for_multi_rules() {
    use crate::app::storage::custom_rule::{CustomRuleAction, CustomRuleMatchType};

    let mut config = generate_base_config(&AppConfig::default());
    let baseline_len = route_rules(&config).len();
    let rules = vec![
        sample_custom_rule(
            CustomRuleMatchType::DomainSuffix,
            "openai.com",
            CustomRuleAction::Proxy,
        ),
        sample_custom_rule(
            CustomRuleMatchType::DomainKeyword,
            "ads",
            CustomRuleAction::Block,
        ),
    ];

    // 连续三次 CRUD 风格 strip+inject，数量必须稳定
    for _ in 0..3 {
        strip_custom_rules(&mut config);
        inject_custom_rules(&mut config, &rules, "手动选择");
        assert_eq!(route_rules(&config).len(), baseline_len + 2);
        assert_eq!(
            route_rules(&config)
                .iter()
                .filter(|r| r.get(CUSTOM_RULE_MARKER).is_some())
                .count(),
            2
        );
    }

    strip_custom_rules(&mut config);
    inject_custom_rules(&mut config, &[], "手动选择");
    assert_eq!(
        route_rules(&config).len(),
        baseline_len,
        "全部删除后应回到基线"
    );
}
