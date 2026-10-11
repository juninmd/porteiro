fn main() {
    let attrs = tauri_build::Attributes::new();
    // Only release builds demand elevation; debug stays launchable from a normal terminal and reports "no admin" in the UI.
    #[cfg(windows)]
    let attrs = if std::env::var("PROFILE").as_deref() == Ok("release") {
        attrs.windows_attributes(
            tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app.manifest")),
        )
    } else {
        attrs
    };
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
