//! Times the window's frames and list operations on a project the `scale` example seeded.
//! Ignored by default; run against an absolute data directory with
//!
//! ```sh
//! AXON_GUI_SCALE_DIR=<data-dir> cargo test --release -p axon-gui --test scale -- --ignored --nocapture
//! ```

use axon_gui::{AxonApp, board::Layout, project::AppData};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, ElementId, Point, TestAppContext, WindowBounds, WindowOptions, base::Root,
    px, size,
};
use std::time::{Duration, Instant};

#[gpui_kit::test]
#[ignore = "measures a seeded project named by AXON_GUI_SCALE_DIR"]
fn frames_and_list_operations(cx: &mut TestAppContext) {
    let dir = std::env::var("AXON_GUI_SCALE_DIR").expect("AXON_GUI_SCALE_DIR");
    let data = AppData::at(dir).unwrap();
    cx.update(axon_gui::init);
    let (window, app) = cx.update(|cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1200.), px(760.)),
            })),
            ..axon_gui::main_window_options(cx)
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| AxonApp::new(data, window, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    let window = window.downcast::<Root>().unwrap();
    let frame = |cx: &mut TestAppContext| -> Duration {
        let started = Instant::now();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();
        started.elapsed()
    };
    // An operation with the frame that shows it.
    let timed = |cx: &mut TestAppContext, operation: &mut dyn FnMut(&mut TestAppContext)| {
        let started = Instant::now();
        operation(cx);
        cx.run_until_parked();
        frame(cx);
        started.elapsed()
    };

    let rows = cx.read(|cx| app.read(cx).explorer().listing().rows.len());
    for _ in 0..3 {
        println!("frame ({rows} rows): {:.2?}", frame(cx));
    }
    let flat = timed(cx, &mut |cx| {
        app.update(cx, |app, cx| app.set_layout(Layout::Flat, cx))
    });
    println!("switch to flat: {flat:.2?}");
    let tree = timed(cx, &mut |cx| {
        app.update(cx, |app, cx| app.set_layout(Layout::Tree, cx))
    });
    println!("switch to tree: {tree:.2?}");
    let toggle = timed(cx, &mut |cx| {
        cx.update_window(window.into(), |_, window, cx| {
            window.click(ElementId::Name("state-Completed".into()), cx)
        })
        .unwrap();
    });
    println!("toggle the Completed state: {toggle:.2?}");
    let id = cx.read(|cx| app.read(cx).explorer().listing().rows[30].id.clone());
    let open = timed(cx, &mut |cx| {
        app.update(cx, |app, cx| app.open_entity(id.clone(), cx))
    });
    println!("open the 31st row: {open:.2?}");
    for _ in 0..3 {
        println!("frame with the detail: {:.2?}", frame(cx));
    }
}
