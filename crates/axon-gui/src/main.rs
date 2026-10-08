use gpui_kit::base::async_util::unbounded;

fn main() {
    // The lock is taken before any window opens. The main window keeps it, and closing that
    // window ends the application, so it is held until the process ends.
    let startup = axon_gui::startup();
    // Links can arrive before the window exists, as when one starts the app; they wait here.
    let (links, received) = unbounded();
    let application = gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        // The app has a single window and no way to reopen it, so closing it ends the app.
        .with_quit_mode(gpui_kit::QuitMode::LastWindowClosed);
    application.on_open_urls(move |urls| {
        // Without a main window, as when the start was refused, there is nothing to open in.
        links.send_blocking(urls).ok();
    });
    application.run(|cx| {
        axon_gui::init(cx);
        let app = match axon_gui::open_startup_window(startup, cx) {
            Ok(app) => app,
            Err(error) => {
                eprintln!("axon-gui: failed to open the window: {error:#}");
                std::process::exit(1);
            }
        };
        if let Some(app) = app {
            let app = app.downgrade();
            cx.spawn(async move |cx| {
                while let Ok(urls) = received.recv().await {
                    cx.update(|cx| axon_gui::open_links(&app, urls, cx));
                }
            })
            .detach();
        }
        cx.activate(true);
    });
}
