use super::*;
use serde::Deserialize;
use serde_json::json;
use std::fs;

#[test]
fn config_util_read_modify_save_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.json");
    fs::write(
        &path,
        r#"{ "route": { "final": "proxy" }, "inbounds": [{"listen_port": 1}] }"#,
    )
    .unwrap();

    let mut util = ConfigUtil::new(path.to_str().unwrap()).unwrap();
    util.modify_property(&["route", "final"], json!("direct"));
    util.update_key(
        vec!["experimental", "clash_api", "external_controller"],
        json!("127.0.0.1:9"),
    );
    util.save().unwrap();
    util.save_to_file().unwrap();

    let loaded: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded["route"]["final"], "direct");
    assert_eq!(
        loaded["experimental"]["clash_api"]["external_controller"],
        "127.0.0.1:9"
    );
}

#[test]
fn config_util_missing_file_errors() {
    let err = ConfigUtil::new("/tmp/definitely-missing-singbox-cfg-xyz.json");
    assert!(err.is_err());
}

#[test]
fn config_util_get_property_as_entity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.json");
    fs::write(&path, r#"{ "log": { "level": "info" } }"#).unwrap();
    let util = ConfigUtil::new(path.to_str().unwrap()).unwrap();

    #[derive(Deserialize)]
    struct Log {
        level: String,
    }
    let log: Log = util.get_property_as_entity(&["log"]).unwrap();
    assert_eq!(log.level, "info");
    assert!(util.get_property_as_entity::<Log>(&["missing"]).is_err());
}
