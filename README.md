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
| `capture-window` | `WindowCapture`(PrintWindow 截单个窗口客户区,遮挡也可截) | 仅 Windows |
| `match-corr` | `CorrMatcher`(corrmatch 的 ZNCC,灰度;调参见 `CorrConfig`) | 跨平台 |
| `parallel` | `RgbMatcher` 按行并行(find / find_all,rayon);`CorrMatcher` 分层并行搜索 | 跨平台 |
| `tracing` | trace 级诊断事件(截图耗时、缓存跳过、命中与否);关闭零开销 | 跨平台 |

```toml
[dependencies]
pixhunt = { version = "0.5", features = ["capture-dxgi", "capture-gdi", "match-corr", "parallel"] }
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
| `Corr` | 有光照/轻微缩放变化、追求稳 | 灰度 ZNCC + 金字塔,只做平移(不搜旋转),较慢但鲁棒 |

## 更多用法 (v0.2)
```rust
use pixhunt::{Finder, CaptureKind, MatchKind, Template, Rect};

// 1) 限定区域:只在该矩形内找,返回的仍是屏幕绝对坐标
let finder = Finder::builder()
    .capture(CaptureKind::Auto)
    .matcher(MatchKind::Rgb { tolerance: 25 })
    .region(Rect::new(100, 80, 640, 480))
    .build()?;

// 2) 多结果:找全部不重叠匹配(重叠自动去重),max=0 表示不限
let all = finder.find_all_on_screen(&tpl, 0)?;

// 3) 批量:只截一屏,一次匹配多张模板(省掉重复截图)
let hits = finder.find_many_on_screen(&[&tpl_a, &tpl_b, &tpl_c])?;
```

匹配器层面也可直接调用 trait 方法:
[`Matcher::find`] 整帧单个、[`Matcher::find_in`] 区域内单个、
[`Matcher::find_all`] 区域内多个。开启 `parallel` 后,`RgbMatcher` 用 rayon 把
扫描按行分到多核,全屏 `find` / `find_all` 在大分辨率下更快(结果与串行完全一致)。

## 等待与窗口 (v0.3)
```rust
use std::time::Duration;
use pixhunt::{Finder, CaptureKind, MatchKind, Template, WindowCapture};

// 4) 轮询等待:等按钮出现(命中即返回,超时返回 None);等遮罩消失同理。
//    配合 DXGI 后端的"静态帧跳过",等待期间几乎零开销。
let m = finder.find_until(&tpl, Duration::from_secs(10), Duration::from_millis(50))?;
let gone = finder.wait_gone(&loading_tpl, Duration::from_secs(30), Duration::from_millis(100))?;

// 5) 窗口级截图(Windows, feature `capture-window`):PrintWindow 渲染客户区,
//    窗口被遮挡也能截;返回坐标为窗口相对。也可用 CaptureKind::Window(hwnd)。
let cap = WindowCapture::from_title("无标题 - 记事本")?;
let mut finder = Finder::new(Box::new(cap), Box::new(pixhunt::RgbMatcher::new(25)));
let m = finder.find_on_screen(&tpl)?; // 相对该窗口客户区的坐标
```

## 与键鼠操作的关系(生态分工)
pixhunt 专注做**眼睛**(截图 + 定位),不做"手"——点击/输入交给
[enigo](https://crates.io/crates/enigo)、[rdev](https://crates.io/crates/rdev)
等成熟跨平台库,坐标就是两者的接口:

```rust,ignore
let m = finder.find_until(&button, Duration::from_secs(10), Duration::from_millis(80))?.unwrap();
enigo.move_mouse(m.x + button.width as i32 / 2, m.y + button.height as i32 / 2, Coordinate::Abs)?;
enigo.button(Button::Left, Direction::Click)?;
```

完整可运行示例见 `examples/wait_and_click.rs`(Windows,`--features capture-gdi`)。

## 颜色搜索与诊断 (v0.4)
```rust
use pixhunt::{ColorSpec, Finder, CaptureKind, MatchKind};

// 6) 颜色范围搜索:没有模板、只有"大概这个颜色"的场景(血条/状态灯/高亮区)。
//    返回 8-连通色块(包围盒+面积),按 (y,x) 排序,min_area 过滤碎点。
let red_lights = finder.find_color_on_screen(&ColorSpec::new(255, 0, 0, 40), 50)?;
for b in red_lights {
    println!("色块 {:?} 面积 {} 中心 {:?}", b.bounds, b.area, b.center());
}
// 也可离线对任意帧用:pixhunt::color::find_blobs(&frame, &spec, region, min_area)
```

开 `tracing` feature 后,pixhunt 在 `pixhunt` target 下输出 trace 级事件:
后端名、截图耗时、静态帧缓存是否命中、扫描结果等——"为什么找不到"先看日志:

```rust,ignore
// 用户侧接一个 subscriber 即可看到
tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).init();
finder.find_on_screen(&tpl)?; // trace: op="find_on_screen" backend="dxgi" changed=false cache_hit=true ...
```

## 性能(参考)
以下数字来自同仓库的 benchmark(1920x1200 / release / 全屏),用来说明**各后端的
相对量级**,并非所有后端都已内置于当前发布版本:

| 组合 | 纯匹配 | 截图 | 端到端(截图+匹配) |
| --- | --- | --- | --- |
| RGB + screenshots | ~5ms | ~54ms | ~57ms |
| RGB + GDI | ~5ms | ~29ms | ~34ms |
| RGB + DXGI | ~5ms | ~7ms | ~12ms |

要点:找图瓶颈主要在**截图**,换更快的后端收益最大;`RgbMatcher` 本身已是毫秒级。

`Corr`(ZNCC)不受截图后端制约,成本在搜索本身(1920x1080 全屏 / release / 64px 模板,
`cargo bench --features match-corr[,parallel] --bench match`):

| 组合 | 热路径(模板已缓存) | 冷启动(含模板编译) |
| --- | --- | --- |
| `Corr` | ~15.7ms | ~16.2ms |
| `Corr` + `parallel`(8 线程) | ~6.2ms | ~6.6ms |

模板只做平移匹配(`compile_unrotated`),不建角度模板库,因此冷启动≈热路径;
开 `parallel` 后 ZNCC 分层并行,约 2.5x(结果仍确定性)。

## ZNCC 调参 (v0.5)
`match-corr` 下用 `MatchKind::CorrWith(CorrConfig { .. })` 调搜索参数(只想用默认值
就继续写 `MatchKind::Corr`):

```rust,ignore
use pixhunt::{CorrConfig, Finder, CaptureKind, MatchKind, Template};

// 已知目标只在附近小范围移动:砍深层金字塔 + 缩小精修 ROI 换低延迟,
// 并用 min_score 把"长得像但不够像"的结果当未命中。
let mut finder = Finder::builder()
    .capture(CaptureKind::Auto)
    .matcher(MatchKind::CorrWith(CorrConfig {
        max_image_levels: 3,
        roi_radius: 4,
        min_score: 0.7,
        ..CorrConfig::default()
    }))
    .build()?;
let m = finder.find_on_screen(&Template::load("btn.png")?)?;
```

| 字段 | 默认 | 调小的收益 / 调大的收益 |
| --- | --- | --- |
| `max_image_levels` | 6 | 粗筛更便宜 / 大位移、轻微缩放更稳 |
| `beam_width` | 8 | 每层候选更少更快 / 遮挡、伪峰多时更稳 |
| `roi_radius` | 8 | 精修扫描范围更小更快 / 容忍层间位移误差更大 |
| `min_score` | 不过滤 | 误命中更少(注意会漏判) |
| `parallel` | 跟随 `parallel` feature | — |

要点:
- 非法值(0、NaN、±inf)会被**夹到安全下限**而不是报错,不会出现"配错就永远找不到"。
- `parallel: true` 只在开了 pixhunt `parallel` feature 时生效——该 feature 会把 `rayon`
  传导给 corrmatch;否则会被归一为 `false`(否则 corrmatch 会在 `validate()` 直接报错,
  表现为静默找不到)。
- `min_score` 是**最终结果**的阈值,在 pixhunt 侧把关,不传给 corrmatch。corrmatch 自己的
  同名字段是**逐金字塔层**的候选门槛,而粗筛层分数天然偏低,拿它当最终阈值会把真命中
  整条链路削空。

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

## 说明:内容由 AI 生成
本仓库的**代码、注释、测试与文档(含本 README)由 AI 编码助手生成或改写**,并经
`cargo test` / `cargo clippy -D warnings` / GitHub Actions 验证。仓库内的性能数字均为
特定机器上的测量值,只能当量级参考。内容**不保证逐行经过人工细读**,请按对待任何
第三方 crate 的方式自行评审后再用于生产。

## 许可
Licensed under either of **MIT** or **Apache License, Version 2.0** at your option.
See `LICENSE-MIT` and `LICENSE-APACHE`.
