use std::process::{exit, Command};

fn main() -> Result<(), std::io::Error> {
    let ret = Command::new("buildsys").arg("build-variant").status()?;
    if !ret.success() {
        exit(1);
    }
    Ok(())
}
