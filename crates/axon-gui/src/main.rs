fn main() {
    // The lock is taken before any window opens and kept until the process ends.
    let startup = axon_gui::startup();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        // The app has a single window and no way to reopen it, so closing it ends the app.
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed)
        .run(|cx| {
            axon_gui::init(cx);
            if let Err(error) = axon_gui::open_startup_window(startup, cx) {
                eprintln!("axon-gui: failed to open the window: {error:#}");
                std::process::exit(1);
            }
            cx.activate(true);
        });
}
