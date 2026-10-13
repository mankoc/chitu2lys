#[cfg(windows)]
fn main() {
    winresource::WindowsResource::new()
        .set_icon("assets/icon.ico")
        .compile()
        .expect("failed to embed the Windows icon");
}

#[cfg(not(windows))]
fn main() {}
