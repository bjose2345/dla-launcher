fn main() {
    // The Chromium build is for Linux; the Android side stays in tauri2/.
    tauri_plugin::Builder::new(&[]).build();
}
