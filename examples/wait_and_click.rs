//! "眼睛 + 手"集成示范:pixhunt 只负责**看**(截图+找图),键鼠点击交给生态库
//! [`enigo`](https://crates.io/crates/enigo)。本示例等一个按钮出现,然后点它中心。
//!
//! 仅 Windows 演示(点击的是屏幕绝对坐标);其他平台本示例只打印说明。
//!
//! ```text
//! cargo run --release --features capture-gdi --example wait_and_click -- path/to/button.png
//! ```

/// 从模板尺寸推算点击点:匹配坐标是模板左上角,加半个模板即中心。
#[cfg(all(windows, feature = "capture-gdi"))]
fn main() -> pixhunt::Result<()> {
    use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
    use pixhunt::{CaptureKind, Finder, MatchKind, Template};
    use std::time::Duration;

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "button.png".to_string());
    let tpl = Template::load(&path)?;

    // 1) 眼睛:GDI 后端轮询等目标出现,10 秒超时(找全屏坐标,便于屏幕点击)。
    let mut finder = Finder::builder()
        .capture(CaptureKind::Gdi)
        .matcher(MatchKind::Rgb { tolerance: 25 })
        .build()?;
    let Some(m) = finder.find_until(&tpl, Duration::from_secs(10), Duration::from_millis(80))?
    else {
        println!("超时未找到 {path}");
        return Ok(());
    };

    // 2) 手:enigo 移动 + 左键点击模板中心。pixhunt 与键鼠解耦,坐标即接口。
    let cx = m.x + tpl.width as i32 / 2;
    let cy = m.y + tpl.height as i32 / 2;
    println!("命中 @ ({}, {}),点击中心 ({cx}, {cy})", m.x, m.y);
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| pixhunt::Error::Capture(format!("enigo init failed: {e}")))?;
    enigo
        .move_mouse(cx, cy, Coordinate::Abs)
        .map_err(|e| pixhunt::Error::Capture(format!("move failed: {e}")))?;
    enigo
        .button(Button::Left, Direction::Click)
        .map_err(|e| pixhunt::Error::Capture(format!("click failed: {e}")))?;
    Ok(())
}

#[cfg(not(all(windows, feature = "capture-gdi")))]
fn main() {
    println!(
        "此演示需要 Windows + --features capture-gdi。\n\
         跨平台做法相同:Finder::find_until 拿到坐标后,交给 enigo/rdev 等键鼠库点击。"
    );
}
