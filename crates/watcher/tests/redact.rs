#![allow(clippy::unwrap_used)]
//! 6.2: what a screen looks like before any agent sees it.

use watcher::redact::{redact, screen_text, REDACTED};

#[test]
fn secret_shapes_and_secret_contexts_are_removed() {
    let secrets = [
        ("export HF_TOKEN=hf_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "hf_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("OPENAI_API_KEY=sk-bbbbbbbbbbbbbbbbbbbbbbbbbbbb python app.py", "sk-bbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ("aws key AKIACCCCCCCCCCCCCCCC", "AKIACCCCCCCCCCCCCCCC"),
        ("Authorization: Bearer ghp_dddddddddddddddddddddddddddddddddddd", "ghp_dddddddddddddddddddddddddddddddddddd"),
        ("git remote add origin https://oauth2:hunter2pass@gitlab.com/team/repo.git", "hunter2pass"),
        ("DATABASE_URL=postgres://admin:Pa55-word@db.internal:5432/app", "Pa55-word"),
        ("Open http://localhost:8888/lab?token=abc123def456", "abc123def456"),
        ("wandb: Appending key for api.wandb.ai to your netrc file: Pa55-netrcvalue1", "Pa55-netrcvalue1"),
        ("MY_SECRET: s3cr3t-value", "s3cr3t-value"),
    ];
    for (line, secret) in secrets {
        let (cleaned, count) = redact(line);
        assert!(!cleaned.contains(secret), "{line} -> {cleaned}");
        assert!(cleaned.contains(REDACTED), "{cleaned}");
        assert!(count >= 1);
    }
    let (cleaned, _) = redact("Open http://localhost:8888/lab?token=abc123def456");
    assert!(cleaned.starts_with("Open http://localhost:8888/lab?token="));
}

#[test]
fn ordinary_training_output_is_untouched() {
    let line = "step 120/500 loss=0.81 lr=3e-4 grad_norm=1.2 | Saved checkpoint to runs/ckpt-120.pt";
    assert_eq!(redact(line), (line.to_string(), 0));
}

#[test]
fn the_screen_is_the_last_lines_without_escapes_or_redraws() {
    let mut raw: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
    raw.push("\u{1b}[32mEpoch 3/10\u{1b}[0m".into());
    raw.push("50%|█████     | 5/10\r100%|██████████| 10/10".into());
    raw.push("export HF_TOKEN=hf_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into());
    raw.push(String::new());
    let (lines, redacted) = screen_text(&raw, 5);
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[2], "Epoch 3/10");
    assert_eq!(lines[3], "100%|██████████| 10/10");
    assert!(lines[4].ends_with(REDACTED));
    assert_eq!(redacted, 1);
    assert!(lines.iter().all(|line| !line.contains('\u{1b}')));
}
