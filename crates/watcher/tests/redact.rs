#![allow(clippy::unwrap_used)]
//! Milestone 5.S output search: secret-shaped values are hidden in results.

use watcher::redact::redact_secrets;

#[test]
fn secret_shaped_values_are_replaced() {
    // Fake token-shaped values, assembled so no literal looks like a real key.
    let github = format!("token gh{}_{}", "p", "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789");
    let tailscale = format!("tailscale up --auth-key {}-auth-k123-ABCDEF", "tskey");
    let cases = [
        (
            "export OPENAI_API_KEY=sk-proj-abc123def456ghi789",
            "export OPENAI_API_KEY=[redacted]",
        ),
        ("password: hunter2", "password: [redacted]"),
        ("DB_PASSWORD=\"Pa55 word\"", "DB_PASSWORD=[redacted]"),
        (
            "curl -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.e30.abc' x",
            "curl -H 'Authorization: Bearer [redacted]' x",
        ),
        (
            "git remote add origin https://oauth2:glpat-xyz@gitlab.com/t/r.git",
            "git remote add origin https://[redacted]@gitlab.com/t/r.git",
        ),
        (github.as_str(), "token [redacted]"),
        (
            "AWS key AKIAIOSFODNN7EXAMPLE in use",
            "AWS key [redacted] in use",
        ),
        (tailscale.as_str(), "tailscale up --auth-key [redacted]"),
        ("-----BEGIN OPENSSH PRIVATE KEY-----", "[redacted]"),
        ("slack xoxb-123-456-abcdef", "slack [redacted]"),
    ];
    for (line, expected) in cases {
        assert_eq!(redact_secrets(line), expected, "{line}");
    }
}

#[test]
fn ordinary_output_is_untouched() {
    for line in [
        "step 120/500 loss=0.81 lr=3e-4 | Saved checkpoint to runs/ckpt-120.pt",
        "RuntimeError: CUDA out of memory. Tried to allocate 2.00 GiB",
        "test result: ok. 12 passed; 0 failed",
        "commit 3f9c1a2b4d5e6f708192a3b4c5d6e7f8091a2b3c",
        "keyboard interrupt received",
    ] {
        assert_eq!(redact_secrets(line), line);
    }
}
