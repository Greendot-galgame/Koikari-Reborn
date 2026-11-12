use koikari_reborn::{
    coverage,
    xp3::{Assets, Store},
};
fn limits(arguments: &[String]) -> Result<coverage::Limits, &'static str> {
    let mut limits = coverage::Limits::default();
    let mut next = 3;
    if let Some(value) = arguments.get(next).filter(|s| !s.starts_with("--")) {
        limits.max_steps = value.parse().map_err(|_| "invalid_limit")?;
        next += 1;
    }
    while let Some(flag) = arguments.get(next) {
        let value = arguments.get(next + 1).ok_or("missing_limit_value")?;
        match flag.as_str() {
            "--max-states" => limits.max_states = value.parse().map_err(|_| "invalid_limit")?,
            "--max-frontier" => limits.max_frontier = value.parse().map_err(|_| "invalid_limit")?,
            "--max-snapshot-bytes" => {
                limits.max_snapshot_bytes = value.parse().map_err(|_| "invalid_limit")?
            }
            "--max-frontier-bytes" => {
                limits.max_frontier_bytes = value.parse().map_err(|_| "invalid_limit")?
            }
            "--max-decisions" => {
                limits.max_decisions = value.parse().map_err(|_| "invalid_limit")?
            }
            "--tick-ms" => limits.tick_ms = value.parse().map_err(|_| "invalid_limit")?,
            _ => return Err("unknown_verification_option"),
        }
        next += 2;
    }
    Ok(limits)
}
fn json_failure(code: &str) -> ! {
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"complete":false,"failures":[code]})
    );
    std::process::exit(2)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    let verification = arguments
        .get(1)
        .is_some_and(|s| matches!(s.as_str(), "verify-all" | "verify-targets"));
    if verification && arguments.get(2).is_none() {
        json_failure("missing_asset_directory");
    }
    let directory = arguments
        .get(2)
        .ok_or("usage: koikari <list|text|verify|walk|verify-all|verify-targets> ASSET_DIRECTORY [NAME|MAX_STEPS] [--max-states N --max-frontier N --max-snapshot-bytes N --max-frontier-bytes N --max-decisions N --tick-ms N]")?;
    let store = match Store::open(directory) {
        Ok(store) => store,
        Err(_) if verification => {
            // Opening errors can contain resource names; never emit arbitrary private error text.
            json_failure("store_open_failed");
        }
        Err(error) => return Err(error.into()),
    };
    match arguments[1].as_str() {
        "list" => {
            for name in store.names() {
                println!("{name}");
            }
        }
        "text" => print!(
            "{}",
            store.text(arguments.get(3).ok_or("missing asset name")?)?
        ),
        "verify" => {
            let name = arguments.get(3).ok_or("missing asset name")?;
            println!("{name}: {} verified bytes", store.bytes(name)?.len());
        }
        "walk" => {
            let mut engine = koikari_reborn::story::Engine::open(std::sync::Arc::new(store))?;
            engine.start()?;
            let limit: u64 = arguments
                .get(3)
                .map(|s| s.parse())
                .transpose()?
                .unwrap_or(10000);
            let mut steps = 0;
            while !engine.state.finished && engine.state.pages < limit && steps < limit {
                if !engine.state.flow.choices.is_empty() || engine.state.scene.video.is_some() {
                    break;
                }
                let before = engine.state.clone();
                if engine.state.flow.wait_ms > 0 {
                    engine.tick(1000)?;
                } else {
                    engine.advance()?;
                }
                steps += 1;
                if engine.state == before {
                    break;
                }
            }
            println!(
                "{} pages, {} layers, finished={}, warnings={:?}",
                engine.state.pages,
                engine.state.scene.layers.len(),
                engine.state.finished,
                engine.state.warnings
            );
        }
        "verify-all" | "verify-targets" => {
            let mut limits = limits(&arguments).unwrap_or_else(|code| json_failure(code));
            limits.target_alternatives = arguments[1] == "verify-targets";
            match coverage::verify_with_limits(std::sync::Arc::new(store), limits) {
                Ok(report) => {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                    if !report.complete && !report.targets_complete {
                        std::process::exit(2);
                    }
                }
                Err(_) => {
                    json_failure("invalid_verification_configuration");
                }
            }
        }
        _ => return Err("unknown command".into()),
    }
    Ok(())
}
