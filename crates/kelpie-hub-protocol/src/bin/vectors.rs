use std::path::Path;

fn main() -> std::io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vectors");
    std::fs::create_dir_all(&dir)?;
    for (name, text) in kelpie_hub_protocol::vectors::render() {
        std::fs::write(dir.join(name), text)?;
        println!("wrote vectors/{name}");
    }
    Ok(())
}
