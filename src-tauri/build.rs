fn main() {
    // Test binaries are linked without an application manifest, so they bind to
    // comctl32 5.82 rather than the side-by-side version 6. tao, underneath
    // Tauri, imports SetWindowSubclass / DefSubclassProc / TaskDialogIndirect,
    // which only version 6 exports, and the loader kills the process with
    // STATUS_ENTRYPOINT_NOT_FOUND before a single test runs.
    //
    // Delay-loading defers those imports to first call. No test creates a
    // window, so the call never happens, and the real app is unaffected: it
    // ships the manifest and gets version 6 as before.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // rustc-link-arg-tests covers only tests/, not the --lib test target,
        // so this has to apply to every binary. The app is unaffected: it ships
        // the manifest, so the first call still resolves against version 6.
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");
    }

    tauri_build::build()
}
