//! Integration-level L3 journeys using only public crate APIs.
mod common;

use app_lib::app::core::kernel_service::KernelChangeImpact;
use app_lib::app::core::proxy_service::{
    apply_system_proxy_for_state, write_inbounds_for_state, ProxyRuntimeState, RecordingSystemProxy,
};
use app_lib::app::core::tun_profile::TunProxyOptions;
use app_lib::app::runtime::change::{plan_runtime_actions, RuntimeApplyOptions, RuntimeChange};
use app_lib::app::singbox::config_generator::{
    generate_base_config, inject_custom_rules, strip_custom_rules,
};
use app_lib::app::storage::custom_rule::{CustomRule, CustomRuleAction, CustomRuleMatchType};
use app_lib::app::storage::state_model::AppConfig;
use chrono::Utc;
use common::E2eEnv;
use std::fs;

/// L3-int-01: public runtime plan API
#[test]
fn l3_int_runtime_plan_subscription_applied() {
    let plan = plan_runtime_actions(
        RuntimeChange::SubscriptionApplied,
        &RuntimeApplyOptions::new("int")
            .patch_active_config(true)
            .restart_if_running(true),
    );
    assert!(plan.apply_proxy_runtime);
    assert_eq!(plan.kernel_impact, KernelChangeImpact::RestartIfRunning);
}

/// L3-int-02: write inbounds + recording proxy without OS side effects
#[tokio::test]
async fn l3_int_proxy_write_and_recording() {
    let env = E2eEnv::new().await;
    E2eEnv::assert_hermetic_env();
    let state = ProxyRuntimeState {
        proxy_port: 16666,
        allow_lan_access: false,
        system_proxy_enabled: true,
        tun_enabled: false,
        system_proxy_bypass: String::new(),
        tun_options: TunProxyOptions::default(),
    };
    write_inbounds_for_state(&env.config_path, &state).unwrap();
    let rec = RecordingSystemProxy::default();
    apply_system_proxy_for_state(&state, &rec).unwrap();
    assert_eq!(rec.enables.lock().unwrap().len(), 1);
}

/// L3-int-03: custom rules inject on disk config
#[tokio::test]
async fn l3_int_custom_rules_inject() {
    let env = E2eEnv::new().await;
    E2eEnv::assert_hermetic_env();
    let rules = vec![CustomRule {
        id: "i1".into(),
        enabled: true,
        match_type: CustomRuleMatchType::Domain,
        payload: "x.test".into(),
        action: CustomRuleAction::Block,
        outbound: None,
        note: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }];
    let mut config = generate_base_config(&AppConfig::default());
    strip_custom_rules(&mut config);
    inject_custom_rules(&mut config, &rules, "手动选择");
    fs::write(
        &env.config_path,
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&env.config_path).unwrap()).unwrap();
    assert!(v["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r.get("__custom_rule__").is_some()));
}
