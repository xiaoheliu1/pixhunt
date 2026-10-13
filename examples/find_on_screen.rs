//! 在屏幕上找一张模板图。
//!
//! 运行:cargo run --release --example find_on_screen -- path\to\template.png
//!
//! 本示例用跨平台后端 `CaptureKind::Monitor`,需要 feature `capture-xcap`(默认开着);
//! 用 `--no-default-features` 剔除截图后端时,它会退化成下面那句提示。

#[cfg(feature = "capture-xcap")]
fn main() -> pixhunt::Result<()> {
    use pixhunt::{CaptureKind, Finder, MatchKind, Template};

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "template.png".to_string());

    let tpl = Template::load(&path)?;
    println!("模板尺寸: {}x{}", tpl.width, tpl.height);

    let mut finder = Finder::builder()
        .capture(CaptureKind::Monitor)
        .matcher(MatchKind::Rgb { tolerance: 25 })
        .build()?;

    match finder.find_on_screen(&tpl)? {
        Some(m) => println!("找到: 左上角=({}, {}), score={}", m.x, m.y, m.score),
        None => println!("未找到(请确认模板内容当前在屏幕上,且分辨率一致)"),
    }
    Ok(())
}

#[cfg(not(feature = "capture-xcap"))]
fn main() {
    println!(
        "此演示需要 xcap 后端(feature `capture-xcap`,默认开启)。\n\
         剔除它仍然可以用匹配核心:自己拿到像素后构一个 Frame,直接调 RgbMatcher::find。"
    );
}
