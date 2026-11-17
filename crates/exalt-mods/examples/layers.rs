//! Builds both layers from a dump, as ExaltDS's setup does: `layers <game.3ds> <out dir>`.
fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, rom, out] = args.as_slice() else { return Err("usage: layers <game.3ds> <out dir>".into()) };
    let title = zakuro_fs::Title::load(rom).map_err(|e| format!("{rom}: {e}"))?;
    let out = std::path::Path::new(out);
    let made = exalt_mods::feet::build(&title, &out.join("feet"), &|_, _| {})?;
    println!("feet for {made} bodies");
    exalt_mods::naga::layer(&title, &out.join("naga"))?;
    println!("Naga's Tear layer written");
    Ok(())
}
