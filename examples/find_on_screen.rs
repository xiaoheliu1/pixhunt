//! 在屏幕上找一张模板图。
//!
//! 运行:cargo run --release --example find_on_screen -- path\to\template.png

use pixhunt::{CaptureKind, Finder, MatchKind, Template};

fn main() -> pixhunt::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "template.png".to_string());

    let tpl = Template::load(&path)?;
    println!("模板尺寸: {}x{}", tpl.width, tpl.height);

    let mut finder = Finder::builder()
        .capture(CaptureKind::Screenshots)
        .matcher(MatchKind::Rgb { tolerance: 25 })
        .build()?;

    match finder.find_on_screen(&tpl)? {
        Some(m) => println!("找到: 左上角=({}, {}), score={}", m.x, m.y, m.score),
        None => println!("未找到(请确认模板内容当前在屏幕上,且分辨率一致)"),
    }
    Ok(())
}
