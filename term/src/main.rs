mod tui;

fn main() -> std::io::Result<()> {
    let _terminal = tui::init()?;
    tui::restore();
    println!("term: terminal init/restore round-trip OK");
    Ok(())
}
