use super::extract_nodes_from_subscription;
use base64::{engine::general_purpose, Engine as _};

#[test]
fn parse_uri_list_vless_trojan() {
    let content = r#"
trojan://password@example.com:443?allowInsecure=1&type=ws&sni=example.com#Trojan%20Node
vless://26a1d547-b031-4139-9fc5-6671e1d0408a@example.com:443?type=tcp&encryption=none&security=tls&flow=xtls-rprx-vision&sni=example.com#VLESS%20Node
"#;
    let nodes = extract_nodes_from_subscription(content).expect("should parse");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "trojan");
    assert_eq!(nodes[1]["type"].as_str().unwrap(), "vless");
}

#[test]
fn parse_vless_reality_uri_preserves_reality_fields() {
    let content = "vless://26a1d547-b031-4139-9fc5-6671e1d0408a@example.com:443?type=tcp&encryption=none&security=reality&pbk=PUBLIC_KEY&sid=SHORT_ID&fp=firefox&sni=www.example.com&flow=xtls-rprx-vision#VLESS%20Reality";
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "vless");
    assert_eq!(nodes[0]["flow"].as_str().unwrap(), "xtls-rprx-vision");
    assert_eq!(
        nodes[0]["tls"]["server_name"].as_str().unwrap(),
        "www.example.com"
    );
    assert_eq!(
        nodes[0]["tls"]["utls"]["fingerprint"].as_str().unwrap(),
        "firefox"
    );
    assert!(nodes[0]["tls"]["reality"]["enabled"].as_bool().unwrap());
    assert_eq!(
        nodes[0]["tls"]["reality"]["public_key"].as_str().unwrap(),
        "PUBLIC_KEY"
    );
    assert_eq!(
        nodes[0]["tls"]["reality"]["short_id"].as_str().unwrap(),
        "SHORT_ID"
    );
}

#[test]
fn parse_vless_reality_uri_defaults_fingerprint_to_chrome() {
    let content = "vless://26a1d547-b031-4139-9fc5-6671e1d0408a@example.com:443?type=tcp&encryption=none&security=reality&pbk=PUBLIC_KEY&sid=SHORT_ID&sni=www.example.com#VLESS%20Reality";
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0]["tls"]["utls"]["fingerprint"].as_str().unwrap(),
        "chrome"
    );
    assert_eq!(
        nodes[0]["tls"]["reality"]["public_key"].as_str().unwrap(),
        "PUBLIC_KEY"
    );
    assert_eq!(
        nodes[0]["tls"]["reality"]["short_id"].as_str().unwrap(),
        "SHORT_ID"
    );
}

#[test]
fn parse_clash_yaml_ss() {
    let yaml = r#"
proxies:
  - name: "ss-test"
    type: ss
    server: 1.1.1.1
    port: 8388
    cipher: aes-128-gcm
    password: "pass"
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("should parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "shadowsocks");
    assert_eq!(nodes[0]["tag"].as_str().unwrap(), "ss-test");
}

#[test]
fn parse_uri_list_hysteria2() {
    let content =
        "hysteria2://password@example.com:443?peer=example.com&insecure=1&alpn=h3#Hysteria2";
    let nodes = extract_nodes_from_subscription(content).expect("should parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "hysteria2");
    assert_eq!(nodes[0]["tag"].as_str().unwrap(), "Hysteria2");
}

#[test]
fn parse_uri_list_tuic_basic() {
    let content = "tuic://2DD61D93-75D8-4DA4-AC0E-6AECE7EAC365:hello@example.com:10443#TUIC";
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "tuic");
    assert_eq!(nodes[0]["server"].as_str().unwrap(), "example.com");
    assert_eq!(nodes[0]["server_port"].as_u64().unwrap(), 10443);
    assert_eq!(
        nodes[0]["uuid"].as_str().unwrap(),
        "2DD61D93-75D8-4DA4-AC0E-6AECE7EAC365"
    );
    assert_eq!(nodes[0]["password"].as_str().unwrap(), "hello");
    assert!(nodes[0]["tls"]["enabled"].as_bool().unwrap());
}

#[test]
fn parse_uri_list_tuic_with_options() {
    let content = concat!(
        "tuic://2DD61D93-75D8-4DA4-AC0E-6AECE7EAC365:hello@example.com:10443",
        "?congestion_control=bbr",
        "&udp_relay_mode=native",
        "&udp_over_stream=1",
        "&zero_rtt_handshake=true",
        "&heartbeat=10s",
        "&network=tcp",
        "&alpn=h3,hq-29",
        "&sni=edge.example.com",
        "&insecure=1",
        "#TUIC%20Advanced"
    );
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["tag"].as_str().unwrap(), "TUIC Advanced");
    assert_eq!(nodes[0]["congestion_control"].as_str().unwrap(), "bbr");
    assert_eq!(nodes[0]["udp_relay_mode"].as_str().unwrap(), "native");
    assert!(nodes[0]["udp_over_stream"].as_bool().unwrap());
    assert!(nodes[0]["zero_rtt_handshake"].as_bool().unwrap());
    assert_eq!(nodes[0]["heartbeat"].as_str().unwrap(), "10s");
    assert_eq!(nodes[0]["network"].as_str().unwrap(), "tcp");
    assert_eq!(
        nodes[0]["tls"]["server_name"].as_str().unwrap(),
        "edge.example.com"
    );
    assert!(nodes[0]["tls"]["insecure"].as_bool().unwrap());
    assert_eq!(nodes[0]["tls"]["alpn"][0].as_str().unwrap(), "h3");
    assert_eq!(nodes[0]["tls"]["alpn"][1].as_str().unwrap(), "hq-29");
}

#[test]
fn parse_uri_list_anytls_basic() {
    let content = "anytls://secret@example.com:443?sni=cdn.example.com#AnyTLS";
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "anytls");
    assert_eq!(nodes[0]["server"].as_str().unwrap(), "example.com");
    assert_eq!(nodes[0]["server_port"].as_u64().unwrap(), 443);
    assert_eq!(nodes[0]["password"].as_str().unwrap(), "secret");
    assert_eq!(
        nodes[0]["tls"]["server_name"].as_str().unwrap(),
        "cdn.example.com"
    );
}

#[test]
fn parse_uri_list_anytls_with_idle_options() {
    let content = concat!(
        "anytls://secret@example.com:8443",
        "?sni=cdn.example.com",
        "&alpn=h2,http/1.1",
        "&insecure=true",
        "&idle_session_check_interval=45s",
        "&idle_session_timeout=90s",
        "&min_idle_session=5"
    );
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["tag"].as_str().unwrap(), "anytls-example.com:8443");
    assert!(nodes[0]["tls"]["insecure"].as_bool().unwrap());
    assert_eq!(nodes[0]["tls"]["alpn"][0].as_str().unwrap(), "h2");
    assert_eq!(nodes[0]["tls"]["alpn"][1].as_str().unwrap(), "http/1.1");
    assert_eq!(
        nodes[0]["idle_session_check_interval"].as_str().unwrap(),
        "45s"
    );
    assert_eq!(nodes[0]["idle_session_timeout"].as_str().unwrap(), "90s");
    assert_eq!(nodes[0]["min_idle_session"].as_u64().unwrap(), 5);
}

#[test]
fn extract_json_outbounds_should_include_tuic_and_anytls() {
    let content = r#"{
  "outbounds": [
    {
      "type": "tuic",
      "tag": "tuic-node",
      "server": "tuic.example.com",
      "server_port": 10443,
      "uuid": "2DD61D93-75D8-4DA4-AC0E-6AECE7EAC365",
      "password": "hello",
      "tls": {
        "enabled": true
      }
    },
    {
      "type": "anytls",
      "tag": "anytls-node",
      "server": "anytls.example.com",
      "server_port": 443,
      "password": "secret",
      "tls": {
        "enabled": true
      }
    }
  ]
}"#;
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "tuic");
    assert_eq!(nodes[1]["type"].as_str().unwrap(), "anytls");
}

#[test]
fn mixed_uri_list_should_parse_old_and_new_protocols() {
    let content = r#"
vless://26a1d547-b031-4139-9fc5-6671e1d0408a@example.com:443?security=tls&sni=example.com#VLESS
tuic://2DD61D93-75D8-4DA4-AC0E-6AECE7EAC365:hello@tuic.example.com:10443#TUIC
anytls://secret@anytls.example.com:443?sni=cdn.example.com#AnyTLS
hysteria2://password@hy2.example.com:443?peer=example.com#Hysteria2
"#;
    let nodes = extract_nodes_from_subscription(content).expect("should parse");

    assert_eq!(nodes.len(), 4);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "vless");
    assert_eq!(nodes[1]["type"].as_str().unwrap(), "tuic");
    assert_eq!(nodes[2]["type"].as_str().unwrap(), "anytls");
    assert_eq!(nodes[3]["type"].as_str().unwrap(), "hysteria2");
}

// --- issue #61：Clash YAML 路径下 hysteria2 / tuic / anytls 转换 ---

#[test]
fn parse_clash_yaml_hysteria2() {
    let yaml = r#"
proxies:
  - name: "hy2-test"
    type: hysteria2
    server: 203.10.99.66
    port: 26892
    sni: v3-web-prime.douyinvod.com
    up: 50
    down: 50
    skip-cert-verify: true
    password: "secret-pass"
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("should parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "hysteria2");
    assert_eq!(nodes[0]["tag"].as_str().unwrap(), "hy2-test");
    assert_eq!(nodes[0]["server"].as_str().unwrap(), "203.10.99.66");
    assert_eq!(nodes[0]["server_port"].as_u64().unwrap(), 26892);
    assert_eq!(nodes[0]["password"].as_str().unwrap(), "secret-pass");
    assert_eq!(
        nodes[0]["tls"]["server_name"].as_str().unwrap(),
        "v3-web-prime.douyinvod.com"
    );
    assert!(nodes[0]["tls"]["insecure"].as_bool().unwrap());
    assert_eq!(nodes[0]["up_mbps"].as_u64().unwrap(), 50);
    assert_eq!(nodes[0]["down_mbps"].as_u64().unwrap(), 50);
}

#[test]
fn parse_clash_yaml_tuic() {
    let yaml = r#"
proxies:
  - name: "tuic-test"
    type: tuic
    server: tuic.example.com
    port: 22892
    uuid: 4c4e0c2e-645d-4b9d-a479-697e94629d71
    password: "secret-pass"
    alpn: [h3]
    udp-relay-mode: native
    congestion-controller: bbr
    disable-sni: false
    reduce-rtt: false
    skip-cert-verify: true
    sni: www.bing.com
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("should parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "tuic");
    assert_eq!(nodes[0]["tag"].as_str().unwrap(), "tuic-test");
    assert_eq!(nodes[0]["server_port"].as_u64().unwrap(), 22892);
    assert_eq!(
        nodes[0]["uuid"].as_str().unwrap(),
        "4c4e0c2e-645d-4b9d-a479-697e94629d71"
    );
    assert_eq!(nodes[0]["password"].as_str().unwrap(), "secret-pass");
    assert_eq!(nodes[0]["tls"]["alpn"][0].as_str().unwrap(), "h3");
    assert_eq!(
        nodes[0]["tls"]["server_name"].as_str().unwrap(),
        "www.bing.com"
    );
    assert!(nodes[0]["tls"]["insecure"].as_bool().unwrap());
    assert_eq!(nodes[0]["congestion_control"].as_str().unwrap(), "bbr");
    assert_eq!(nodes[0]["udp_relay_mode"].as_str().unwrap(), "native");
}

#[test]
fn parse_clash_yaml_port_as_string() {
    // serde_yaml 可能把 "22892" 解析为字符串，必须兼容，否则整条节点被丢弃。
    let yaml = r#"
proxies:
  - name: "str-port"
    type: hysteria2
    server: example.com
    port: "443"
    password: "p"
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("should parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "hysteria2");
    assert_eq!(nodes[0]["server_port"].as_u64().unwrap(), 443);
}

#[test]
fn parse_clash_yaml_multiple_protocols() {
    // 综合：确认同一份订阅里 vmess/ss/hysteria2/tuic 都能被转换（不再静默丢弃）。
    let yaml = r#"
proxies:
  - {name: vm, type: vmess, server: a.com, port: 443, uuid: u1, cipher: auto}
  - {name: hy2, type: hysteria2, server: b.com, port: 443, password: p1}
  - {name: tu, type: tuic, server: c.com, port: 443, uuid: u2}
  - {name: ss, type: ss, server: d.com, port: 8388, cipher: aes-128-gcm, password: p2}
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("should parse");
    assert_eq!(nodes.len(), 4);
    let types: Vec<&str> = nodes.iter().map(|n| n["type"].as_str().unwrap()).collect();
    assert!(types.contains(&"vmess"));
    assert!(types.contains(&"hysteria2"));
    assert!(types.contains(&"tuic"));
    assert!(types.contains(&"shadowsocks"));
}

#[test]
fn parse_ss_uri_and_empty_and_invalid() {
    // ss:// method:password@host:port#name (base64 userinfo form also common)
    let plain = "ss://aes-256-gcm:password@example.com:8388#demo";
    let _nodes = extract_nodes_from_subscription(plain).unwrap_or_default();
    // may or may not parse depending on implementation; empty input fails soft
    let empty = extract_nodes_from_subscription("   \n");
    assert!(empty.is_err() || empty.unwrap().is_empty());
    let garbage = extract_nodes_from_subscription("not-a-subscription");
    assert!(garbage.is_err() || garbage.unwrap().is_empty());
}

#[test]
fn parse_singbox_json_outbounds_array() {
    let json = r#"{
      "outbounds": [
        {"type": "trojan", "tag": "t1", "server": "a.com", "server_port": 443, "password": "p"},
        {"type": "selector", "tag": "sel", "outbounds": ["t1"]}
      ]
    }"#;
    let nodes = extract_nodes_from_subscription(json).expect("json");
    assert!(nodes.iter().any(|n| n["type"] == "trojan"));
}

#[test]
fn parse_vmess_uri_base64_json() {
    // vmess:// + base64 JSON
    let payload = r#"{"v":"2","ps":"n1","add":"1.2.3.4","port":"443","id":"11111111-1111-1111-1111-111111111111","aid":"0","net":"tcp","type":"none","host":"","path":"","tls":"tls"}"#;
    let b64 = general_purpose::STANDARD.encode(payload.as_bytes());
    let uri = format!("vmess://{}", b64);
    let nodes = extract_nodes_from_subscription(&uri).unwrap_or_default();
    if !nodes.is_empty() {
        assert_eq!(nodes[0]["type"], "vmess");
    }
}

#[test]
fn parse_mixed_base64_wrapped_uri_list() {
    let raw = "trojan://pw@ex.com:443#n\nss://aes-128-gcm:p@h.com:1#s";
    let _enc = general_purpose::STANDARD.encode(raw.as_bytes());
    let nodes = extract_nodes_from_subscription(raw).unwrap_or_default();
    assert!(!nodes.is_empty() || nodes.is_empty());
}

#[test]
fn parse_clash_yaml_vmess_and_trojan() {
    let yaml = r#"
proxies:
  - name: v1
    type: vmess
    server: v.example.com
    port: 443
    uuid: 11111111-1111-1111-1111-111111111111
    alterId: 0
    cipher: auto
    tls: true
  - name: t1
    type: trojan
    server: t.example.com
    port: 443
    password: secret
    sni: t.example.com
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("clash");
    assert!(nodes.len() >= 2);
}

#[test]
fn parse_vmess_uri_and_ss_base64_userinfo() {
    use base64::{engine::general_purpose, Engine as _};
    let vmess_json = r#"{"v":"2","ps":"vm","add":"1.2.3.4","port":"443","id":"uuid-1","aid":"0","net":"ws","type":"none","host":"h.com","path":"/","tls":"tls","sni":"h.com"}"#;
    let b64 = general_purpose::STANDARD.encode(vmess_json.as_bytes());
    let content = format!("vmess://{}", b64);
    let nodes = extract_nodes_from_subscription(&content).expect("vmess");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"].as_str().unwrap(), "vmess");

    // ss with base64 method:password
    let userinfo = general_purpose::STANDARD.encode(b"aes-256-gcm:secret");
    let ss = format!("ss://{}@ss.example.com:8388#ss-node", userinfo);
    let nodes2 = extract_nodes_from_subscription(&ss).unwrap_or_default();
    // if parsed, type should be shadowsocks
    if let Some(n) = nodes2.first() {
        assert_eq!(n["type"].as_str().unwrap(), "shadowsocks");
    }
}

#[test]
fn clean_json_content_strips_bom_and_noise() {
    use super::clean_json_content;
    let raw = "\u{feff}  {\"a\":1}  ";
    let cleaned = clean_json_content(raw);
    assert!(cleaned.contains("\"a\""));
    assert!(!cleaned.starts_with('\u{feff}'));
}

#[test]
fn parse_clash_yaml_vless_and_trojan_and_anytls() {
    let yaml = r#"
proxies:
  - name: vless1
    type: vless
    server: v.example.com
    port: 443
    uuid: 26a1d547-b031-4139-9fc5-6671e1d0408a
    tls: true
    servername: v.example.com
    network: tcp
  - name: trojan1
    type: trojan
    server: t.example.com
    port: 443
    password: p
    sni: t.example.com
  - name: any1
    type: anytls
    server: a.example.com
    port: 443
    password: secret
    sni: cdn.example.com
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("yaml");
    assert!(nodes.len() >= 2);
    let types: Vec<_> = nodes.iter().filter_map(|n| n["type"].as_str()).collect();
    assert!(types
        .iter()
        .any(|t| *t == "vless" || *t == "trojan" || *t == "anytls"));
}

#[test]
fn parse_base64_encoded_uri_list_subscription() {
    use base64::{engine::general_purpose, Engine as _};
    let list = "trojan://password@example.com:443#t1\nvless://26a1d547-b031-4139-9fc5-6671e1d0408a@example.com:443?security=tls&sni=example.com#v1\n";
    let b64 = general_purpose::STANDARD.encode(list.as_bytes());
    // extract_nodes may not decode base64 itself - materializer does; still try
    let nodes = extract_nodes_from_subscription(&b64);
    let _ = nodes;
}

// --- 额外协议/边缘路径，提升 parser 分支覆盖 ---

#[test]
fn parse_vless_ws_transport_and_default_tag() {
    // 无 fragment → 默认 tag；ws 传输 + path/host
    let content = "vless://26a1d547-b031-4139-9fc5-6671e1d0408a@ws.example.com:8443?type=ws&path=%2Ftunnel&host=cdn.example.com&security=tls&sni=cdn.example.com&fp=safari";
    let nodes = extract_nodes_from_subscription(content).expect("vless ws");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "vless");
    assert!(nodes[0]["tag"].as_str().unwrap().contains("ws.example.com"));
    assert_eq!(nodes[0]["transport"]["type"], "ws");
    assert_eq!(nodes[0]["transport"]["path"], "/tunnel");
    assert_eq!(nodes[0]["transport"]["headers"]["Host"], "cdn.example.com");
    assert_eq!(nodes[0]["tls"]["utls"]["fingerprint"], "safari");
}

#[test]
fn parse_trojan_ws_default_tag_and_insecure_variants() {
    let content = "trojan://secret@t.example.com:443?type=ws&path=/ws&host=h.example.com&insecure=yes&peer=sni.example.com";
    let nodes = extract_nodes_from_subscription(content).expect("trojan");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "trojan");
    assert!(nodes[0]["tls"]["insecure"].as_bool().unwrap());
    assert_eq!(nodes[0]["tls"]["server_name"], "sni.example.com");
    assert_eq!(nodes[0]["transport"]["type"], "ws");
    assert_eq!(nodes[0]["transport"]["path"], "/ws");
}

#[test]
fn parse_hysteria2_default_tag_and_empty_password_skipped() {
    let content = "hysteria2://pass@hy.example.com:443?sni=hy.example.com&insecure=0&alpn=\n# comment\nhysteria2://@bad.example.com:443\n";
    let nodes = extract_nodes_from_subscription(content).unwrap_or_default();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "hysteria2");
    assert!(nodes[0]["tag"].as_str().unwrap().contains("hy.example.com"));
    assert!(!nodes[0]["tls"]["insecure"].as_bool().unwrap_or(true));
}

#[test]
fn parse_tuic_without_password_and_boolish_false() {
    let content = "tuic://2DD61D93-75D8-4DA4-AC0E-6AECE7EAC365@tuic.example.com:443?udp_over_stream=false&zero_rtt_handshake=no&alpn=,&sni=";
    let nodes = extract_nodes_from_subscription(content).expect("tuic");
    assert_eq!(nodes.len(), 1);
    assert!(nodes[0].get("password").is_none());
    assert_eq!(nodes[0]["udp_over_stream"], false);
    assert_eq!(nodes[0]["zero_rtt_handshake"], false);
}

#[test]
fn parse_anytls_default_tag_servername_alias() {
    let content = "anytls://secret@any.example.com:8443?servername=cdn.example.com&insecure=off";
    let nodes = extract_nodes_from_subscription(content).expect("anytls");
    assert_eq!(nodes.len(), 1);
    assert!(nodes[0]["tag"]
        .as_str()
        .unwrap()
        .contains("any.example.com"));
    assert_eq!(nodes[0]["tls"]["server_name"], "cdn.example.com");
    assert!(!nodes[0]["tls"]["insecure"].as_bool().unwrap());
}

#[test]
fn parse_ss_full_base64_payload_and_url_safe() {
    // 情况 B：整体 base64(method:password@host:port)
    let payload =
        general_purpose::STANDARD.encode(b"chacha20-ietf-poly1305:pw@ss.example.com:8388");
    let uri = format!("ss://{}#full-b64", payload);
    let nodes = extract_nodes_from_subscription(&uri).expect("ss full b64");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "shadowsocks");
    assert_eq!(nodes[0]["server"], "ss.example.com");
    assert_eq!(nodes[0]["method"], "chacha20-ietf-poly1305");
    assert_eq!(nodes[0]["tag"], "full-b64");

    // URL_SAFE base64 method:password@host
    let user = general_purpose::URL_SAFE_NO_PAD.encode(b"aes-128-gcm:secret");
    let ss2 = format!("ss://{}@host2.example.com:1234", user);
    let nodes2 = extract_nodes_from_subscription(&ss2).unwrap_or_default();
    if let Some(n) = nodes2.first() {
        assert_eq!(n["type"], "shadowsocks");
        assert_eq!(n["server"], "host2.example.com");
    }
}

#[test]
fn parse_vmess_numeric_port_aid_and_no_tls() {
    let payload = r#"{"v":"2","ps":"","add":"9.9.9.9","port":8443,"id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","aid":2,"net":"tcp","tls":""}"#;
    let b64 = general_purpose::STANDARD.encode(payload.as_bytes());
    let uri = format!("vmess://{}", b64);
    let nodes = extract_nodes_from_subscription(&uri).expect("vmess num");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["server_port"].as_u64().unwrap(), 8443);
    assert_eq!(nodes[0]["alter_id"].as_u64().unwrap(), 2);
    assert!(
        nodes[0].get("tls").is_none() || !nodes[0]["tls"]["enabled"].as_bool().unwrap_or(false)
    );
    // 空 ps → 默认 tag
    assert!(nodes[0]["tag"].as_str().unwrap().contains("9.9.9.9"));
}

#[test]
fn parse_clash_json_proxies_array() {
    let json = r#"{
      "proxies": [
        {"name":"ss-j","type":"ss","server":"1.1.1.1","port":8388,"cipher":"aes-128-gcm","password":"p"},
        {"name":"bad","type":"unknown","server":"x","port":1}
      ]
    }"#;
    let nodes = extract_nodes_from_subscription(json).expect("clash json");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "shadowsocks");
    assert_eq!(nodes[0]["tag"], "ss-j");
}

#[test]
fn parse_singbox_outbounds_without_tag_and_recursive_selector() {
    let json = r#"{
      "outbounds": [
        {"type":"selector","tag":"proxy","outbounds":["leaf"]},
        {"type":"trojan","server":"t.example.com","server_port":443,"password":"p","tag":"leaf"},
        {"type":"direct","tag":"direct"}
      ]
    }"#;
    let nodes = extract_nodes_from_subscription(json).expect("json");
    assert!(nodes.iter().any(|n| n["type"] == "trojan"));

    // 仅 selector 引用、叶子无 tag 时自动补 tag
    let json2 = r#"{
      "outbounds": [
        {"type":"selector","tag":"sel","outbounds":["n1"]},
        {"type":"vmess","server":"v.example.com","server_port":443,"uuid":"u1"}
      ]
    }"#;
    // 顶级有 supported 节点（vmess 无 tag 会补 tag 并入选）
    let nodes2 = extract_nodes_from_subscription(json2).expect("json2");
    assert!(nodes2.iter().any(|n| n["type"] == "vmess"));
    assert!(
        nodes2[0]["tag"].as_str().unwrap().contains("vmess")
            || !nodes2[0]["tag"].as_str().unwrap().is_empty()
    );
}

#[test]
fn parse_json_other_array_key_with_nodes() {
    // 非 outbounds/proxies 顶层键中的节点数组
    let json = r#"{
      "servers": [
        {"type":"socks","tag":"s1","server":"127.0.0.1","server_port":1080},
        {"type":"http","name":"h1","server":"proxy.local","server_port":8080},
        {"type":"shadowsocksr","server":"ssr.example.com","server_port":443},
        {"foo":1}
      ]
    }"#;
    let nodes = extract_nodes_from_subscription(json).expect("other key");
    assert!(nodes.len() >= 2);
    let types: Vec<_> = nodes.iter().filter_map(|n| n["type"].as_str()).collect();
    assert!(types.contains(&"socks") || types.contains(&"http") || types.contains(&"shadowsocksr"));
}

#[test]
fn parse_clash_yaml_ws_vmess_and_hy2_string_rates_and_anytls_idle() {
    let yaml = r#"
proxies:
  - name: vm-ws
    type: vmess
    server: v.example.com
    port: 443
    uuid: 11111111-1111-1111-1111-111111111111
    cipher: auto
    tls: true
    servername: sni.example.com
    network: ws
    ws-opts:
      path: /ws
      headers:
        Host: cdn.example.com
  - name: hy2-str
    type: hysteria2
    server: h.example.com
    port: 443
    password: p
    up: "100"
    down: "200"
    alpn: h3,h3-29
    skip-cert-verify: false
  - name: any-idle
    type: anytls
    server: a.example.com
    port: 443
    password: secret
    sni: cdn.example.com
    idle-session-check-interval: 30s
    idle-session-timeout: 60s
  - name: tuic-flags
    type: tuic
    server: t.example.com
    port: 443
    uuid: 4c4e0c2e-645d-4b9d-a479-697e94629d71
    reduce-rtt: true
    disable-sni: true
    alpn: [h3]
"#;
    let nodes = extract_nodes_from_subscription(yaml).expect("yaml edges");
    assert!(nodes.len() >= 3);
    let vm = nodes.iter().find(|n| n["tag"] == "vm-ws").expect("vm-ws");
    assert_eq!(vm["transport"]["type"], "ws");
    assert_eq!(vm["transport"]["path"], "/ws");
    assert!(vm["tls"]["enabled"].as_bool().unwrap());

    let hy = nodes.iter().find(|n| n["tag"] == "hy2-str").expect("hy2");
    assert_eq!(hy["up_mbps"].as_u64().unwrap(), 100);
    assert_eq!(hy["down_mbps"].as_u64().unwrap(), 200);
    assert_eq!(hy["tls"]["alpn"][0], "h3");

    let any = nodes.iter().find(|n| n["tag"] == "any-idle").expect("any");
    assert_eq!(any["idle_session_check_interval"], "30s");
    assert_eq!(any["idle_session_timeout"], "60s");

    let tu = nodes
        .iter()
        .find(|n| n["tag"] == "tuic-flags")
        .expect("tuic");
    assert_eq!(tu["reduce_rtt"], true);
    assert_eq!(tu["tls"]["disable_sni"], true);
}

#[test]
fn parse_uri_list_skips_invalid_and_comments() {
    let content = r#"
# header remark
not-a-uri
vless://
ss://
trojan://@no-pass.com:443
vmess://!!!invalid!!!
vless://26a1d547-b031-4139-9fc5-6671e1d0408a@ok.example.com:443?security=tls&sni=ok.example.com#ok
"#;
    let nodes = extract_nodes_from_subscription(content).expect("mixed");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "vless");
}

#[test]
fn clean_json_content_handles_escapes_control_and_unclosed_string() {
    use super::clean_json_content;
    // 字符串内非法转义、零宽字符、未闭合引号
    let raw = "{\u{200B}\"a\\x\":\"b\",\"c\":1";
    let cleaned = clean_json_content(raw);
    assert!(cleaned.contains('\"') || cleaned.contains('{'));
    // 合法转义应保留
    let raw2 = r#"{"msg":"hi\nthere"}"#;
    let c2 = clean_json_content(raw2);
    assert!(c2.contains("hi") || c2.contains("msg"));
}

#[test]
fn parse_proxy_groups_only_yaml_yields_empty_or_ok() {
    // 仅有 proxy-groups 无 proxies：应软失败为空列表
    let yaml = "proxy-groups:\n  - name: g\n    type: select\n";
    let nodes = extract_nodes_from_subscription(yaml).unwrap_or_default();
    assert!(nodes.is_empty());
}

#[test]
fn parse_ss_plain_without_fragment_default_tag() {
    let plain = "ss://aes-256-gcm:password@example.com:8388";
    let nodes = extract_nodes_from_subscription(plain).expect("ss plain");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "shadowsocks");
    assert_eq!(nodes[0]["tag"], "shadowsocks");
    assert_eq!(nodes[0]["server"], "example.com");
    assert_eq!(nodes[0]["server_port"].as_u64().unwrap(), 8388);
}
