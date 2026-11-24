//! `sentinald`: the Sentinal daemon. `sentinald keeper` is the process that
//! holds one terminal across daemon restarts (T5); the daemon starts it.

fn main() {
    let mode: Vec<String> = std::env::args().skip(1).take(2).collect();
    match mode
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["keeper", "stop-all"] => match daemon::keeper::stop_all() {
            Ok(count) => {
                println!("asked {count} keeper(s) to stop");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("sentinald keeper stop-all: {error}");
                std::process::exit(1);
            }
        },
        ["keeper", ..] => std::process::exit(daemon::keeper::process::main()),
        _ => {}
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Error: {error:?}");
            std::process::exit(1);
        }
    };
    let result = runtime.block_on(daemon::entry::run());
    if let Err(error) = &result {
        eprintln!("Error: {error:?}");
    }
    // Exit now: dropping the runtime would wait for blocking tasks (PTY
    // readers, log tails), which can outlive SIGTERM and stall launchd/systemd.
    std::process::exit(i32::from(result.is_err()));
}
