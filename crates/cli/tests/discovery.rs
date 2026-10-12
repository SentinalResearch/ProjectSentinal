//! Tailnet control discovery (4.E §4) with an injected transport that records
//! every probe: only online `.ts.net` peers with Tailscale addresses, at most
//! 8 at once and 64 in all, and only answers that name the probed origin.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::discovery::{discover, Probe, Transport};
use serde_json::{json, Value};
use std::{
    net::IpAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};

const CONTROL_ID: &str = "01K2C7RQ9M3X8V6T4N2P0SDB5A";

fn answer(host: &str, name: &str) -> Probe {
    Probe::Body(
        json!({
            "sentinal": "control", "schema_version": 1,
            "control_machine_id": CONTROL_ID, "display_name": name,
            "origin": format!("https://{host}"), "release": "0.1.18",
            "protocol": {"min": 1, "max": 1},
            "pairing": {"approval": false, "code": true}
        })
        .to_string()
        .into_bytes(),
    )
}

#[derive(Default)]
struct Fake {
    peers: Vec<Value>,
    answers: Vec<(String, Probe)>,
    delay: Duration,
    probed: Mutex<Vec<(String, IpAddr)>>,
    active: AtomicUsize,
    most_active: AtomicUsize,
}

impl Fake {
    fn peer(&mut self, host: &str, ip: &str, online: bool) {
        self.peers.push(json!({
            "DNSName": format!("{host}."), "Online": online, "TailscaleIPs": [ip]
        }));
    }
}

impl Transport for Fake {
    fn status(&self) -> Result<Value, String> {
        let peers: serde_json::Map<String, Value> = self
            .peers
            .iter()
            .enumerate()
            .map(|(index, peer)| (format!("nodekey:{index}"), peer.clone()))
            .collect();
        Ok(json!({"BackendState": "Running", "Peer": peers}))
    }

    fn probe(&self, host: &str, ip: IpAddr) -> Probe {
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_active.fetch_max(now, Ordering::SeqCst);
        self.probed.lock().unwrap().push((host.to_string(), ip));
        thread::sleep(self.delay);
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.answers
            .iter()
            .find(|(answered, _)| answered == host)
            .map(|(_, probe)| probe.clone())
            .unwrap_or(Probe::NoAnswer)
    }
}

#[test]
fn only_online_tailnet_peers_are_probed_and_only_valid_controls_are_listed() {
    let mut fake = Fake::default();
    fake.peer("pilot.example.ts.net", "100.101.1.1", true);
    fake.peer("lab.example.ts.net", "100.101.1.2", true);
    fake.peer("old.example.ts.net", "100.101.1.3", true);
    fake.peer("quiet.example.ts.net", "100.101.1.4", true);
    fake.peer("liar.example.ts.net", "100.101.1.5", true);
    fake.peer("asleep.example.ts.net", "100.101.1.6", false);
    fake.peer("lan.example.com", "192.168.1.7", true);
    fake.peer("public.example.ts.net", "8.8.8.8", true);
    fake.answers = vec![
        (
            "pilot.example.ts.net".into(),
            answer("pilot.example.ts.net", "Owner's Sentinal"),
        ),
        (
            "lab.example.ts.net".into(),
            answer("lab.example.ts.net", "Owner's Sentinal"),
        ),
        ("old.example.ts.net".into(), Probe::NotFound),
        // Answers for another origin: a rebinding or misconfigured proxy.
        (
            "liar.example.ts.net".into(),
            answer("pilot.example.ts.net", "Fake"),
        ),
    ];
    let found = discover(&fake, Duration::from_secs(6)).unwrap();

    let mut probed: Vec<String> = fake
        .probed
        .lock()
        .unwrap()
        .iter()
        .map(|(host, _)| host.clone())
        .collect();
    probed.sort();
    assert_eq!(
        probed,
        vec![
            "lab.example.ts.net",
            "liar.example.ts.net",
            "old.example.ts.net",
            "pilot.example.ts.net",
            "quiet.example.ts.net"
        ]
    );
    for (host, ip) in fake.probed.lock().unwrap().iter() {
        let expected = fake
            .peers
            .iter()
            .find(|peer| peer["DNSName"] == format!("{host}."))
            .unwrap();
        assert_eq!(
            expected["TailscaleIPs"][0],
            ip.to_string(),
            "probe pinned to the peer's address"
        );
    }
    let origins: Vec<&str> = found
        .controls
        .iter()
        .map(|control| control.discovery.origin.as_str())
        .collect();
    assert_eq!(
        origins,
        vec!["https://lab.example.ts.net", "https://pilot.example.ts.net"]
    );
    assert_eq!(found.older, vec!["https://old.example.ts.net".to_string()]);
    assert_eq!(found.checked, 5);
    assert_eq!(found.unanswered, 2, "quiet and the liar");
}

#[test]
fn probes_are_bounded_in_concurrency_count_and_time() {
    let mut fake = Fake {
        delay: Duration::from_millis(40),
        ..Fake::default()
    };
    for index in 0..100 {
        fake.peer(
            &format!("peer{index}.example.ts.net"),
            &format!("100.101.2.{}", index + 1),
            true,
        );
    }
    let found = discover(&fake, Duration::from_secs(6)).unwrap();
    assert_eq!(fake.probed.lock().unwrap().len(), 64);
    assert!(fake.most_active.load(Ordering::SeqCst) <= 8);
    assert_eq!(found.checked, 64);

    let slow = Fake {
        delay: Duration::from_millis(200),
        peers: fake.peers.clone(),
        ..Fake::default()
    };
    let started = std::time::Instant::now();
    discover(&slow, Duration::from_millis(300)).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
    assert!(slow.probed.lock().unwrap().len() < 64);
}

#[test]
fn oversized_or_malformed_answers_are_ignored() {
    let mut fake = Fake::default();
    fake.peer("big.example.ts.net", "100.101.3.1", true);
    fake.peer("junk.example.ts.net", "100.101.3.2", true);
    fake.answers = vec![
        ("big.example.ts.net".into(), Probe::Body(vec![b' '; 5000])),
        (
            "junk.example.ts.net".into(),
            Probe::Body(b"<html>".to_vec()),
        ),
    ];
    let found = discover(&fake, Duration::from_secs(6)).unwrap();
    assert!(found.controls.is_empty());
    assert_eq!(found.unanswered, 2);
}
