//! Renders the app icon (`crates/app/icon/live.svg`) at every size macOS asks for and packs them
//! into `live.icns` beside it with `iconutil`. `cargo run -p pina-live --example icon`

use std::{path::Path, process::Command};

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("icon");
    let svg = std::fs::read(dir.join("live.svg")).unwrap();
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).unwrap();
    let render = |px: u32| {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px).unwrap();
        let s = px as f32 / tree.size().width();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(s, s),
            &mut pixmap.as_mut(),
        );
        pixmap
    };
    let set = std::env::temp_dir().join("pina-live.iconset");
    let _ = std::fs::remove_dir_all(&set);
    std::fs::create_dir_all(&set).unwrap();
    for size in [16u32, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            render(size * scale)
                .save_png(set.join(format!("icon_{size}x{size}{suffix}.png")))
                .unwrap();
        }
    }
    render(1024).save_png(dir.join("live.png")).unwrap();
    let ok = Command::new("iconutil")
        .args(["-c", "icns", "-o"])
        .arg(dir.join("live.icns"))
        .arg(&set)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "iconutil failed");
    println!("{}", dir.join("live.icns").display());
}
