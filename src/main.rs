use koikari_reborn::xp3::{Assets, Store};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    let directory = arguments
        .get(2)
        .ok_or("usage: koikari <list|text|verify> ASSET_DIRECTORY [NAME]")?;
    let store = Store::open(directory)?;
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
            while !engine.state.finished && engine.state.pages < limit {
                engine.advance()?;
            }
            println!(
                "{} pages, {} layers, finished={}, warnings={:?}",
                engine.state.pages,
                engine.state.scene.layers.len(),
                engine.state.finished,
                engine.state.warnings
            );
        }
        _ => return Err("unknown command".into()),
    }
    Ok(())
}
