//! Validation rules for Machine identity, display names, codes, and URLs.

use protocol::link::{
    normalize_pairing_code, validate_control_url, validate_display_name, ControlUrlPolicy,
    PAIRING_CODE_ALPHABET,
};
use protocol::{LocalRequest, MachineId, Secret};

#[test]
fn ulid_machine_ids_are_validated() {
    assert!(MachineId("01J9Z3K8V6Q2T7M4X5C0RNB8DA".into()).is_ulid());
    for bad in [
        "",
        "rtx-pc",
        "01J9Z3K8V6Q2T7M4X5C0RNB8D",
        "01J9Z3K8V6Q2T7M4X5C0RNB8DAX",
        "01J9Z3K8V6Q2T7M4X5C0RNB8DI",
        "81J9Z3K8V6Q2T7M4X5C0RNB8DA",
        "01j9z3k8v6q2t7m4x5c0rnb8da",
    ] {
        assert!(!MachineId(bad.into()).is_ulid(), "{bad}");
    }
}

#[test]
fn display_names_are_trimmed_bounded_and_free_of_controls() {
    assert_eq!(validate_display_name("  RTX PC  ").unwrap(), "RTX PC");
    assert_eq!(validate_display_name("Пк 🚀").unwrap(), "Пк 🚀");
    assert_eq!(
        validate_display_name(&"x".repeat(64))
            .unwrap()
            .chars()
            .count(),
        64
    );
    for bad in [
        String::new(),
        "   ".into(),
        "x".repeat(65),
        "tab\there".into(),
        "line\nbreak".into(),
        "esc\u{1b}[31m".into(),
        "del\u{7f}".into(),
        "c1\u{85}".into(),
        "bidi\u{202e}cba".into(),
        "iso\u{2066}x".into(),
    ] {
        assert!(validate_display_name(&bad).is_err(), "{bad:?}");
    }
}

#[test]
fn pairing_codes_normalize_crockford_input() {
    assert_eq!(PAIRING_CODE_ALPHABET.len(), 32);
    assert_eq!(normalize_pairing_code("7kq4m-2xz9c").unwrap(), "7KQ4M2XZ9C");
    assert_eq!(
        normalize_pairing_code(" 7KQ4M 2XZ9C ").unwrap(),
        "7KQ4M2XZ9C"
    );
    assert_eq!(normalize_pairing_code("OOOOO-IIILL").unwrap(), "0000011111");
    for bad in [
        "",
        "7KQ4M-2XZ9",
        "7KQ4M-2XZ9CC",
        "7KQ4M-2XZ9U",
        "7KQ4M_2XZ9C",
        "7KQ4M-2XZ9!",
    ] {
        assert!(normalize_pairing_code(bad).is_err(), "{bad}");
    }
}

#[test]
fn control_urls_are_tailnet_https_origins_unless_development() {
    let production = ControlUrlPolicy::Production;
    assert_eq!(
        validate_control_url("https://Sentinal-Control.example.ts.net/", production).unwrap(),
        "https://sentinal-control.example.ts.net"
    );
    for bad in [
        "http://sentinal-control.example.ts.net",
        "https://sentinal-control.example.ts.net:8443",
        "https://user@sentinal-control.example.ts.net",
        "https://sentinal-control.example.ts.net/api",
        "https://sentinal-control.example.ts.net?code=x",
        "https://sentinal-control.example.ts.net#x",
        "https://sentinal-control.example.ts.net.evil.com",
        "https://ts.net",
        "https://100.64.0.1",
        "ftp://x.ts.net",
    ] {
        assert!(validate_control_url(bad, production).is_err(), "{bad}");
    }
    let development = ControlUrlPolicy::Development;
    assert_eq!(
        validate_control_url("http://127.0.0.1:7421", development).unwrap(),
        "http://127.0.0.1:7421"
    );
    assert_eq!(
        validate_control_url("http://100.93.101.56:7420", development).unwrap(),
        "http://100.93.101.56:7420"
    );
    assert!(validate_control_url("http://192.168.1.5:7420", development).is_err());
    assert!(validate_control_url("http://127.0.0.1:7421/?code=x", development).is_err());
}

#[test]
fn connect_control_validates_url_name_and_code_shape() {
    let request = |url: &str, code: &str, name: Option<&str>| LocalRequest::ConnectControl {
        control_url: url.into(),
        pairing_code: Secret::new(code),
        display_name: name.map(Into::into),
    };
    assert!(
        request("https://c.example.ts.net", "7KQ4M-2XZ9C", Some("RTX"))
            .validate()
            .is_ok()
    );
    assert!(request("https://c.example.ts.net", "7KQ4M-2XZ9C", None)
        .validate()
        .is_ok());
    let error = request("https://c.example.ts.net", "7KQ4M-2XZ9", None)
        .validate()
        .unwrap_err()
        .to_string();
    assert!(!error.contains("7KQ4M"), "error leaks the code: {error}");
    assert!(
        request("https://c.example.ts.net", "7KQ4M-2XZ9C", Some("bad\nname"))
            .validate()
            .is_err()
    );
}

#[test]
fn secret_display_and_debug_are_redacted() {
    let secret = Secret::new("hunter2-secret");
    assert_eq!(format!("{secret}"), "[redacted]");
    assert_eq!(format!("{secret:?}"), "[redacted]");
    assert_eq!(secret.expose(), "hunter2-secret");
    assert_eq!(
        serde_json::to_string(&secret).unwrap(),
        "\"hunter2-secret\""
    );
}
