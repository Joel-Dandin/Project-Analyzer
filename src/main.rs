//! Desktop entry point; scanning and persistence remain in the library.

mod ui;

fn main() -> anyhow::Result<()> {
    ui::run()
}
