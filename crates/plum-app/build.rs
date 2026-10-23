fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // Embed the icon in the .exe so Explorer, shortcuts and the taskbar pick it up.
    #[cfg(windows)]
    winresource::WindowsResource::new().set_icon("assets/icon.ico").compile().expect("embedding the Windows icon resource");
}
