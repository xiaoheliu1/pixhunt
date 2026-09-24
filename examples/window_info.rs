//! 用 PrintWindow 截取一个真实窗口,并"自截自找"验证坐标一致性。
//! 需 Windows + `--features capture-window`。
//!
//! ```text
//! cargo run --release --features capture-window --example window_info "Program Manager"
//! ```

#[cfg(all(windows, feature = "capture-window"))]
fn main() -> pixhunt::Result<()> {
    use pixhunt::{Capture, Matcher, PixelFormat, RgbMatcher, Template, WindowCapture};

    let title = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Program Manager".to_string());
    let mut cap = WindowCapture::from_title(&title)?;
    let frame = cap.grab()?;
    println!(
        "窗口 {title:?} 客户区: {}x{} (BGRA {} 字节)",
        frame.width,
        frame.height,
        frame.pixels.len()
    );
    assert_eq!(frame.format, PixelFormat::Bgra8);

    // 自截自找:从帧中心抠一块 32x32 当模板,应精确回到原位。
    let (w, h) = (frame.width, frame.height);
    let (cx, cy) = (w / 2, h / 2);
    let (ro, go, bo) = frame.rgb_offsets();
    let mut rgb = Vec::with_capacity(32 * 32 * 3);
    for y in cy..cy + 32 {
        for x in cx..cx + 32 {
            let i = (y * w + x) * 4;
            rgb.extend_from_slice(&[
                frame.pixels[i + ro],
                frame.pixels[i + go],
                frame.pixels[i + bo],
            ]);
        }
    }
    let tpl = Template::from_rgb(rgb, 32, 32);
    let m = RgbMatcher::new(0)
        .find(&frame, &tpl)
        .expect("自截自找应命中");
    println!("模板回找命中 @ ({}, {}),期望 ({}, {})", m.x, m.y, cx, cy);
    assert_eq!((m.x, m.y), (cx as i32, cy as i32));
    println!("OK: capture-window 后端工作正常");
    Ok(())
}

#[cfg(not(all(windows, feature = "capture-window")))]
fn main() {
    eprintln!("此示例需要 Windows 平台并用 --features capture-window 构建");
}
