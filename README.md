# pixhunt

[![CI](https://github.com/xiaoheliu1/pixhunt/actions/workflows/ci.yml/badge.svg)](https://github.com/xiaoheliu1/pixhunt/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/pixhunt.svg)](https://crates.io/crates/pixhunt)
[![docs.rs](https://img.shields.io/docsrs/pixhunt)](https://docs.rs/pixhunt)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#许可)

> 快速、低依赖的**屏幕找图**库:截一帧屏幕,在其中定位一张小图(模板)的坐标。
> 纯 Rust,无需 OpenCV。适合自动化测试、脚本辅助、UI 定位等。

## 它做什么
给一张小图 `template.png`,`pixhunt` 告诉你在当前屏幕的哪个位置、有多像:

```rust
use pixhunt::{Finder, CaptureKind, MatchKind, Template};

let tpl = Template::load("template.png")?;
let mut finder = Finder::builder()
    .capture(CaptureKind::Screenshots)         // 截图后端
    .matcher(MatchKind::Rgb { tolerance: 25 }) // 找法
    .build()?;

if let Some(m) = finder.find_on_screen(&tpl)? {
    println!("在 ({}, {}), 相似度 {}", m.x, m.y, m.score);
}
```

## 设计:两个可插拔"插座"
- **`Capture`** 负责"怎么拿到画面"。
- **`Matcher`** 负责"怎么找模板"。

上层只跟"插座"打交道,新增后端/算法不改上层代码。

## 功能开关 (Cargo features)
| feature | 提供 | 平台 |
| --- | --- | --- |
| (默认) | `ScreenshotsCapture` + `RgbMatcher` | 跨平台 |
| `capture-gdi` | `GdiCapture`(复用 DC + BitBlt,输出 BGRA) | 仅 Windows |
| `capture-dxgi` | `DxgiCapture`(桌面复制,GPU 取帧) | 仅 Windows |
| `match-corr` | `CorrMatcher`(corrmatch 的 ZNCC,灰度) | 跨平台 |

```toml
[dependencies]
pixhunt = { version = "0.1", features = ["capture-dxgi", "capture-gdi", "match-corr"] }
```

启用后 `CaptureKind` 多出 `Gdi` / `Dxgi` / `Auto`(`Auto` 依次试 DXGI → GDI → screenshots),
`MatchKind` 多出 `Corr`:

```rust
let mut finder = Finder::builder()
    .capture(CaptureKind::Auto)     // 自动选最快可用的截图后端
    .matcher(MatchKind::Rgb { tolerance: 25 })
    .build()?;
```

## 后端与算法怎么选
| 截图后端 | 特点 | 相对成本 |
| --- | --- | --- |
| `Screenshots` | 跨平台保底 | 高(每帧重建对象 + 多次全帧拷贝) |
| `Gdi` | 复用对象 + BitBlt + 免翻转 | 中(约为 screenshots 的一半) |
| `Dxgi` | 桌面复制,GPU 取帧 | 低(纯读回可到个位数 ms) |

| 匹配算法 | 适合 | 特点 |
| --- | --- | --- |
| `Rgb` | 屏幕内容与模板几乎一致、追求速度 | 不转灰度、锚点 + 逐像素早失败,通道序自适应(RGBA/BGRA) |
| `Corr` | 有光照/轻微缩放变化、追求稳 | 灰度 ZNCC + 金字塔,较慢但鲁棒 |

## 性能(参考)
以下数字来自同仓库的 benchmark(1920x1200 / release / 全屏),用来说明**各后端的
相对量级**,并非所有后端都已内置于当前发布版本:

| 组合 | 纯匹配 | 截图 | 端到端(截图+匹配) |
| --- | --- | --- | --- |
| RGB + screenshots | ~5ms | ~54ms | ~57ms |
| RGB + GDI | ~5ms | ~29ms | ~34ms |
| RGB + DXGI | ~5ms | ~7ms | ~12ms |

要点:找图瓶颈主要在**截图**,换更快的后端收益最大;`RgbMatcher` 本身已是毫秒级。

## 运行示例
```bash
# 默认(screenshots 后端)
cargo run --release --example find_on_screen -- path/to/template.png
# 用 Windows 快后端 + 自动选择
cargo run --release --features capture-dxgi,capture-gdi --example find_on_screen -- path/to/template.png
```

## 注意
- **分辨率 / DPI**:模板与截图需同一分辨率尺度,否则找不到。
- **截图 ≠ 匹配**:两者是分开计时/分开的步骤,别把截图耗时算进算法。
- **DXGI 限制**:RDP / 锁屏 / 无 GPU 时不可用,`CaptureKind::Auto` 会自动回退。
- 帧字节序:GDI / DXGI 产出 BGRA,screenshots 产出 RGBA;`RgbMatcher` 按帧的
  `PixelFormat` 自动映射通道,无需你手动转换。

## 许可
Licensed under either of **MIT** or **Apache License, Version 2.0** at your option.
See `LICENSE-MIT` and `LICENSE-APACHE`.
