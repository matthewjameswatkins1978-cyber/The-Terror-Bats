//! Windows executable resources: embed the canonical Terror Bats icon.
//! Non-Windows targets skip resource compilation entirely.

#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/brand/terrorbats.ico");
    res.set("ProductName", "The Terror Bats Framework");
    res.set(
        "FileDescription",
        "terrorbats — falsification and assurance framework",
    );
    res.set("OriginalFilename", "terrorbats.exe");
    if let Err(e) = res.compile() {
        eprintln!("warning: Windows resource compile failed: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
