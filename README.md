# pixhunt

[![CI](https://github.com/xiaoheliu1/pixhunt/actions/workflows/ci.yml/badge.svg)](https://github.com/xiaoheliu1/pixhunt/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/pixhunt.svg)](https://crates.io/crates/pixhunt)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#许可)

> 快速的**屏幕找图**库:截一帧屏幕,在其中定位一张小图(模板)的坐标。
> 纯 Rust,无需 OpenCV。适合自动化测试、脚本辅助、UI 定位等。

## 它做什么
给一张小图 `template.png`,`pixhunt` 告诉你在当前屏幕的哪个位置(用 `Corr` 时还给一个
相似度分数):

```rust
use pixhunt::{Finder, CaptureKind, MatchKind, Template};

let tpl = Template::load("template.png")?;
let mut finder = Finder::builder()
    .capture(CaptureKind::Monitor)             // 截图后端
    .matcher(MatchKind::Rgb { tolerance: 25 }) // 找法
    .build()?;

if let Some(m) = finder.find_on_screen(&tpl)? {
    // 注意:`Rgb` 是"容差内逐像素全对才算命中"的判定,它的 m.score 恒为 1.0;
    // 想要真正的相似度分数(0.0..=1.0)请用 MatchKind::Corr。
    // 另:`Some` 只说明"至少有一处"。命中不止一处时它给的是最上、最左的那个,
    // 不告诉你还有第二个 —— 要判唯一用 finder.find_all_on_screen(&tpl, 2)。
    let (cx, cy) = m.center(&tpl); // 要点击的是中心,不是左上角
    println!("命中 @ ({}, {}),中心 ({}, {})", m.x, m.y, cx, cy);
}
```

## 文档
- **[使用说明书(从零开始)](docs/GUIDE.md)** —— 面向没写过 Rust 的读者:环境准备、最小语法课、
  核心概念、**全部对外 API 逐个详解**(真实签名 + 参数 + 返回 + 坑)、任务配方、性能调参、
  报错排查、API 速查表。
- `cargo doc --no-deps --open` —— 本地生成的 API 参考(以源码注释为准)。
- `examples/` 四个可运行示例的用法见说明书 12.6。

## 设计:两个可插拔"插座"
- **`Capture`** 负责"怎么拿到画面"。
- **`Matcher`** 负责"怎么找模板"。

上层只跟"插座"打交道,新增后端/算法不改上层代码。

## 功能开关 (Cargo features)
| feature | 提供 | 平台 |
| --- | --- | --- |
| `capture-xcap` **默认开** | `XCapCapture`、`CaptureKind::Monitor`(基于 xcap) | 跨平台 |
| (不开任何后端) | 只剩匹配核心:`Frame` / `Template` / `RgbMatcher` / 颜色搜索 —— 画面由宿主自己抓、用 `Frame::rgba8` 注入,xcap 不进依赖树 | 跨平台 |
| `capture-gdi` | `GdiCapture`(复用 DC + BitBlt,输出 BGRA) | 仅 Windows |
| `capture-dxgi` | `DxgiCapture`(桌面复制,GPU 取帧) | 仅 Windows |
| `capture-window` | `WindowCapture`(PrintWindow 截单个窗口客户区,遮挡也可截)+ `CaptureKind::WindowByTitle` | 仅 Windows |
| `match-corr` | `CorrMatcher`(corrmatch 的 ZNCC,灰度;调参见 `CorrConfig`) | 跨平台 |
| `parallel` | `RgbMatcher` 按行并行(find / find_all,rayon);`CorrMatcher` 分层搜索。**例外**:平坦画面的 `find_all` 里并行帮不上忙,库里会自己退回串行流式(见 v0.8.2 变更) | 跨平台 |
| `tracing` | trace 级诊断事件(截图耗时、缓存跳过、命中与否、长轮询每 5 秒一条进度);关闭零开销 | 跨平台 |

```toml
[dependencies]
pixhunt = { version = "0.10", features = ["capture-dxgi", "capture-gdi", "match-corr", "parallel"] }
```

启用后 `CaptureKind` 多出 `Gdi` / `Dxgi` / `Auto`(`Auto` 依次试 DXGI → GDI → xcap;
GDI 的构造不返回错误,所以开了 `capture-gdi` 就永远走不到 xcap 那一档),
`MatchKind` 多出 `Corr`:

```rust
let mut finder = Finder::builder()
    .capture(CaptureKind::Auto)     // 自动选最快可用的截图后端
    .matcher(MatchKind::Rgb { tolerance: 25 })
    .build()?;
```

## 后端与算法怎么选
| 截图后端 | 特点 | 成本(1920x1200 实测) |
| --- | --- | --- |
| `Monitor`(xcap) | 跨平台保底;唯一支持**区域直抓**的后端 | 全屏 ~34ms / 400x300 区域 ~16.5ms |
| `Gdi` | 复用 DC + BitBlt + 免翻转 | 全屏 ~32ms |
| `Dxgi` | 桌面复制,GPU 取帧 + 静态帧跳过 | 全屏 ~16ms(无新帧时更低) |

(以上为同一台 1920x1200 桌面、`--release`、连续取帧的中位数;`xcap` / `Gdi` 走 GDI
路径,量级接近,`xcap` 的价值在于跨平台 + 区域直抓。)

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

// 2) 多结果:找全部不重叠匹配(重叠自动去重),按 (y, x) 升序;max=0 表示不限。
//    ⚠️ 平坦画面(纯色背景 + 纯色小模板)上"每个位置都是命中",别随手写 0:
//    1920x1200 纯色帧 + 8x8 纯色模板实测返回 36000 个。给个真实上限(比如 20)能提前停止扫描。
let all = finder.find_all_on_screen(&tpl, 20)?;
for m in &all {
    let (cx, cy) = m.center(&tpl); // 多结果要点中心时用它,不用自己折算
    println!("命中 @ ({}, {}) → 点击 ({cx}, {cy})", m.x, m.y);
}

// 3) 批量:只截一屏,一次匹配多张模板(省掉重复截图)
let hits = finder.find_many_on_screen(&[&tpl_a, &tpl_b, &tpl_c])?;
```

匹配器层面也可直接调用 trait 方法:
[`Matcher::find`] 整帧单个、[`Matcher::find_in`] 区域内单个、
[`Matcher::find_all`] 区域内多个。开启 `parallel` 后,`RgbMatcher` 用 rayon 把
扫描按行分到多核,全屏 `find` / `find_all` 在大分辨率下更快(结果与串行完全一致)。
⚠️ 唯一例外:**平坦**(到处都匹配)画面上的 `find_all` —— 重叠抑制必须按 `(y, x)` 顺序
流式进行,并行反而会慢几个数量级,库里会自己退回串行(见 v0.8.2 变更)。

## 等待与窗口 (v0.3)
```rust
use std::time::Duration;
use pixhunt::{Finder, CaptureKind, MatchKind, Template, WindowCapture};

// 4) 轮询等待:等按钮出现(命中即返回,超时返回 None);等遮罩消失同理。
//    timeout 没有上限,写多久就等多久(传 Duration::MAX = 永不超时,不会 panic);
//    但里面没有中途叫停的通道——要能随时收手就自己拿 find_on_screen 组循环。
//    静态画面会被跳过搜索:DXGI 后端自己上报,或设了 region 时由库里逐字节比对区域帧。
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
let (cx, cy) = m.center(&button); // 匹配坐标是左上角,center() 折算成点击的中心
enigo.move_mouse(cx, cy, Coordinate::Abs)?;
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

> ⚠️ **下面所有数字都是 `--release` 构建实测。** debug 构建(`cargo run` / `cargo build`
> 默认)慢一个数量级:同一台机器、同一份 1920x1080 合成场景,全屏扫一遍
> debug ~60ms / release ~4ms。别拿 debug 的耗时评价这个库。
>
> 另:`Cargo.toml` 里的 `[profile.release]`(`lto`、`codegen-units = 1`)**只对在本仓库里
> 跑示例/基准生效**。你依赖 pixhunt 时,优化级别由你自己项目的 `[profile.release]` 决定
> ——想吃到同样性能,请在你项目根 `Cargo.toml` 里也写上这三行(见 `docs/GUIDE.md` 10.4)。

同一台 8 逻辑核机器、`--release` 实测。截图为 1920x1200 真实桌面的连续取帧中位数,
纯匹配为 `cargo bench --bench match`(1920x1080 合成帧;单目标用 64px 模板,`find_all`
用 48px):

| 组合 | 纯匹配 | 截图 | 端到端(截图+匹配) |
| --- | --- | --- | --- |
| RGB + `Monitor`(xcap) | 串行 ~2.8ms / `parallel` ~1.8ms | ~34ms | ~36ms |
| RGB + `Gdi` | 同上 | ~32ms | ~34ms |
| RGB + `Dxgi` | 同上 | ~16ms(静态桌面更低) | ~18ms |

要点:找图瓶颈主要在**截图**,换更快的后端收益最大;`RgbMatcher` 本身已是毫秒级。
限定区域 + `Monitor` 会走**区域直抓**(400x300 实测截图 ~16.5ms,与"截全屏再裁剪"
逐字节一致),同一模板端到端从 ~47ms 降到 ~20ms。

掩码与差分另有两个实测数:`rgb_find_masked_1080p`(64px 模板掩掉 6% 像素)~2.9ms,与同尺寸
无掩码的 ~2.8ms 只在抖动内 —— 掩码买的是正确性、不是速度(旧文档写的"慢 18%"本机复现
不出来,已删);`diff_since_last_400x300_on_1080p` ~3.4ms
(基线缓冲复用之后;复用前 ~5.6ms)。

`Corr`(ZNCC)不受截图后端制约,成本在搜索本身(同样 `cargo bench --features
match-corr[,parallel] --bench match`):

| 组合 | 热路径(模板已缓存) | 冷启动(含模板编译) |
| --- | --- | --- |
| `Corr` | ~13.4ms | ~13.7ms |
| `Corr` + `parallel`(8 逻辑核) | ~6.4ms | ~6.5ms |

模板只做平移匹配(`compile_unrotated`),不建角度模板库,因此冷启动≈热路径;
开 `parallel` 后 ZNCC 分层并行,约 2x(结果仍确定性)。

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

## v0.10 变更

次版本号级别,含一**个破坏性改动**(见下面"升级到 v0.10")。只动一件事:`RgbMatcher` 的
预筛锚点怎么取。命中结果与顺序逐个不变。

为什么改:

- 旧版固定取"中心 + 四角"那 5 个点。横幅 / 标签这类模板,中心与四角全部落在同一片
  底色上 —— 5 个锚点读的是**同一个颜色**,等于只有 1 个在工作;大容差下几乎每个位置都能
  过预筛,于是每个位置都要付一次整窗核对。实测靶子(1272x2800 真机截帧 + 594x136 大图
  模板,满屏相似横幅的列表页)容差 40 时旧版一次查找要 **3.4 秒**。
- 另一头,5x5 这类小模板上那 5 个点会重合(内缩 2 像素 + 坐标去重),实际只剩 1 个锚点。

改成什么:

- 锚点在**模板构造时**就算好并存进 `Template`(`load` / `from_rgb` / `from_rgba` / `with_mask`
  四个入口都会),一次查找零分配。锚点与容差无关,所以改 `tolerance` 不重算。
- 选点:局部对比度 + 颜色稀有度各 0.5 打分,按分数**降序**排列(第一枪最值钱);网格保证
  散布(每格**至多**一个);锚点之间颜色互斥,专治"5 个点读同一个颜色"。
- 个数按**可见区面积**反推,地板是旧版的 5 个、上限 12:预筛的代价是按位置算的内存取数
  (每个锚点落在屏幕不同行,基本等于一次 cache miss),不是越多越好。
- 内缩边距分轴自适应(不让小模板塌成一点,也不让细长条的边距被另一轴拖着压成 0);
  候选池等间隔子采样到 25 万点以内(构造成本不会因模板巨大而变成几百毫秒)。
- 掩码在打分**之前**生效:被掩像素不进候选池、不进颜色直方图、不参与邻域对比度。被掩处的
  RGB 是 png 里原样存着的残留值,混进直方图会凭空造出一个巨大的黑桶,把真正偏暗的边缘点
  误判成"常见色、低分"。
- 全程无 RNG、无时间相关状态:分格与并列都按线性下标(即 `(y, x)` 升序)打破,同一张
  模板永远得到同一组锚点。

不变量(零漏检的依据):锚点全是模板里**未被掩**的真实像素,所以"锚点全过"是"整窗匹配"的
**必要条件** —— 预筛只可能白比,不可能误杀真匹配。整窗核对一行未改,容差语义不变。

实测(真机截帧 1272x2800 + 594x136 模板,`--release --features parallel`,帧按 BGRA 建 =
与安卓注入同口径;2 枪预热 + 12 次取中位。可用 `cargo run --release --features parallel --
example ab_prefilter -- <帧 png> <模板 png>` 复现):

| 场景 | 容差 | v0.9 | v0.10 |
| --- | --- | --- | --- |
| 全屏扫不到(MISS) | 0 | 3.22ms | 2.91ms |
| | 10 | 3.10ms | 3.31ms |
| | 25 | **927ms** | 3.25ms |
| | 40 | **3140ms** | 3.23ms |
| 列表页命中 (42,545) | 0 | **1210ms** | 1.16ms |
| | 10 | 2183ms | 1.40ms |
| | 25 | 3711ms | 1.72ms |
| | 40 | 3471ms | 1.39ms |

命中坐标前后完全一致。合成横幅场景的单测里量得更直接:旧那 5 个点在 240x120 的底色画面上
放过 **18194** 个错位置,新一组只放过 **13** 个。

表里那两行分别是"一张全屏扫不到的 1272x2800 截帧"与"一张满屏相似横幅的列表页截帧(命中
(42,545))"。同一协议重跑过一遍:MISS 那格 2.8~2.9ms、命中那格 0.93~1.0ms,与表格同量级
(表格记的是当时那一次,不是多次里挑的最优)。

机制更正(跟原先的预期不一样,按实测改口):原本以为"排序 + 去同色会把每个位置的取数从
2~3 次降到 1 次,连底价(平坦画面扫满)也会跟着降"。实测**底价几乎没动**(3.22 → 2.91ms,
那一格就是全屏扫满,内存取数的物理底价)。上表 `tol=0` 从 1210ms 到 1.16ms 靠的是另一件事:
列表页满屏相似横幅,旧版即使容差 0 也有成万个位置能过预筛、每个都要整窗核对,新锚点把它们
在第一枪就拒掉了。**省下的是错候选的整窗核对,不是取数次数。**

crate 自带 bench 的口径(同一台机器,HEAD 快照与改动版**交替**跑、共 4 轮,
`--sample-size 50 --measurement-time 3`):`rgb_find_fullscreen_1080p` / `rgb_find_masked_1080p` /
`rgb_find_all_fullscreen_1080p` 均值差 **+3~5%**,而轮间抖动本身就有 ±7%,所以判定为"无量级
变化"(这个场景是**纯色模板** + 渐变帧,模板自己单色时任何选点法都拿不到不同色的锚点,而且
首锚点与旧版是同一个点);`rgb_find_all_flat_1080p_max1` 稳定**快 ~12%**(698ns → 611ns)。
⚠️ criterion 自己报的 "Performance has regressed +39%" 是拿**陈旧基线**比出来的,不是这一轮
对拍的结果。

测试 / 文档:

- 新增 16 条单测(锚点自适应 13 条 + 轮询 3 条),钉住这几件事:零漏检(真匹配处
  在 tol 0/1/10/25/40/64 都要过预筛)、全掩模板语义、小模板不塌成一点、个数地板与上限、
  锚点不得落在被掩像素上、被掩像素不污染稀有度、确定性、超大模板子采样,以及上面那组
  "新旧锚点放过多少个错位置"的对照。`cargo test --all-features` 共 **85 条通过**
  (`--release` 84 条,少的那条是本来就带 `#[cfg(debug_assertions)]` 的 frame 溢出测试,
  发布构建里不参与编译)。
- `docs/GUIDE.md` 同步:8.4.1 / 8.4.2 / 8.4.3 写明构造会算锚点与 `with_mask` 会重算,8.6.2 的
  工作原理改成新选点法(并写明纯色模板那条退化场景为什么不受影响),迁移表补 v0.9→v0.10。
- 新增常驻测量入口 `examples/ab_prefilter.rs`(只用公开 API,所以能同时在改动前/后的代码上跑)。

轮询等待(`find_until` / `wait_gone`)动了三处:

- **每等满 5 秒打一条进度 trace**(`op = "poll_progress"`,带"已等了多久"与"拍了几次")。
  等很久时,这是唯一能区分"还在转"和"已经卡死"的东西;不开 feature `tracing` 时整块不
  参与编译,连轮次计数都不存在。
- 把三件事写明了(代码早就如此,只是没写在脸上):`timeout` **没有任何上限**,写多久就等
  多久;`Duration::ZERO` 是只看一眼(`interval` 传 ZERO 是圈间不额外睡);轮询中**没有中途
  叫停的通道**——只有"命中""到点""截图报错"三个出口,要"等很久又能随时收手"得自己拿
  `find_on_screen` 组循环。`docs/GUIDE.md` 开头那句"等一个按钮出现(最多等 10 秒)"是把
  示例当上限写了,已改;依赖示例里残留的 `0.9` 也全部跟到 `0.10`。
- 上面两句都补了能**当场跑**的测试:`Duration::ZERO` 两条用"数拍了多少次"的 mock 钉住
  "确实只拍一帧";进度日志那条装了个自己写的假订阅器(不多一个依赖),收到
  `op = "poll_progress"` 才算过。为了让它能在一眨眼里跑完,5 秒那个阈值从常量改成了
  `poll` 的入参(两个公开入口仍传 5 秒);阈值留在常量里的话,测试就得真等满 5 秒。
  这条测试不是写了就算:把 `op` 改名后它**确实会红**(一开始用"包含"比对时不会,已改成全等)。

## 升级到 v0.10 (breaking)

破坏面只有一处,就在 `Template`:

| v0.9 | v0.10 |
| --- | --- |
| 可以写 `Template { rgb, width, height, mask }` | 结构体内多出两个**不可见**的预计算字段(预筛锚点表、可见区外接框),这种**字面量构造编不过**。改走 `Template::from_rgb(...)` / `from_rgba(...)` / `load(...)` 即可;读字段(`tpl.width` 等)不受影响 |
| `Template` 是"纯数据",clone 不贵 | 构造时算一次锚点(代价与面积成正比,常规尺寸是微秒量级);**宿主侧请缓存 `Arc<Template>`**,不要每次找图 `load` 一遍。安卓侧 `TEMPLATES` 已同步改成 `HashMap<PathBuf, Arc<Template>>`,去掉每次 `.cloned()` 白拷一张模板像素(另一个宿主 `rhai_demo` 同形改动也已跟上) |
| 改掩码只是换个比较集合 | `with_mask` 现在会**重算**锚点与外接框(这是正确性要求:锚点必须是还参与比较的像素) |

行为上不需要你改任何代码:命中集合、坐标、`(y, x)` 顺序、`score` 口径、`tolerance` 语义全部
不变。真机 A/B 的上表里前后报的是同一个坐标。

## v0.9 变更

次版本号级别:新增一个可插拔能力 + 一批修复加固。**默认开启 `capture-xcap`,桌面用户的体验与 0.8.x 完全一致**。

新增能力:

- **xcap 后端改为可选**:新增 feature `capture-xcap`(默认开)。`default-features = false` 时
  只剩匹配核心(`Frame` / `Template` / `RgbMatcher` / 颜色搜索),xcap 不进依赖树,画面由
  宿主自己抓并用 `Frame::rgba8` 注入 —— 面向安卓 / iOS 等**没有截屏后端**的平台(如 Kotlin
  MediaProjection 抓帧喂进来)。`CaptureKind::Monitor` / `XCapCapture` 仅在此开关打开时存在。
  ⚠️ 语义变化:0.8.2 里 xcap 是硬依赖、`default = []`,`default-features = false` 与开默认毫无
  区别(拿不到 xcap 的唯一方式以前不存在);现在 `default-features = false` 会真的剔除 xcap。
  此前没有任何配置会因此从"能用"变"不能用",只是新开了一条 core-only 用法。

修复:

- **GDI 后端改分辨率那一帧的尺寸同步顺序**:`GdiCapture::grab` / `grab_into` 此前先读旧尺寸
  再触发内部重建,导致分辨率变化的那一帧 `Frame::width/height` 与 `pixels` 缓冲不是同一个尺寸
  —— 新尺寸更大时是一张静默错位的画面(找图莫名全灭),新尺寸更小时按下标读到缓冲之外。
  改成先重建、再取尺寸(与 `WindowCapture` 的正确写法一致),并加 `debug_assert` 兜住。
  ⚠️ 这一条是**代码推断**:要真改一次分辨率才能复现,本机没构造出该场景,未跑通前后对比。
- **`Template::load` 挡住解压炸弹**:先把文件头里的宽高拿来做 `checked_mul` + 4000 万像素上限
  校验,不超线才真正解码;超限返回 `Err(Error::Image(ImageError::Limits(..)))`。此前直接按
  文件头声明的尺寸分配,一张几百 KB 却写巨大尺寸的 PNG 能把内存打死。

文档 / 门禁:

- **DXGI 多显示器选屏语义更正**:`DxgiCapture` 取的是 `EnumAdapters(0)` + `EnumOutputs(0)`,
  单显卡时那就是主屏,多显卡(核显 + 独显)时**不保证**是主显示器;README/`docs/GUIDE.md` 补上
  "各后端到底抓哪一块屏"的说明与副屏排查提示。
- **CI 覆盖面补齐**:`--no-default-features` 现在跑**全量 test(含 doctest)**,不再用 `--lib` 绕过
  (`src/lib.rs` 顶部示例已用 feature cfg 兜住 `CaptureKind::Monitor`);新增单后端 clippy、
  Linux `--all-features` clippy、MSRV 的 no-default 构建,以及安卓(aarch64 / x86_64)交叉 check。

## v0.8.2 变更

修订号级别:没有破坏性改动,`find_all` 的**命中集合与顺序和 v0.8.1 逐个字节相同**(测试里
与朴素 O(n²) 参照逐字节对照过)。改的是"知情权"和一件算法:

- **`find_on_screen` 现在明写:命中不止一处时,你从返回值里看不出来**。它返回扫描序
  `(y, x)` 的第一个(最上、同样靠上时最靠左)——这个选择是确定的,串行/并行、换后端都给
  同一个坐标——但 `Some` 只表示"至少有一处"。要判唯一就数到 2:

  ```rust
  match finder.find_all_on_screen(&tpl, 2)?.len() {
      0 => {}                                       // 没找到
      1 => {}                                       // 唯一,可以放心点
      _ => {}                                       // 不止一处:缩小 region 或换更有特征的模板
  }
  ```

  ⚠️ `MatchKind::Corr` 下这个守卫**无效**:`CorrMatcher` 没有覆写 `find_all`,永远只返回
  1 个,于是它看起来"永远唯一"。
- **`find_all` 的早停从"行"粒度细化到"列"粒度**:重叠覆盖的判定挪到逐像素验证**之前**
  (被前面的命中盖住的列直接不验证)、攒够 `max` 当场收工、整行/整块都被覆盖就一行都不扫。
  平坦画面因此再掉一到四个数量级(1920x1200 纯色帧,`--release`,同一台机器上同一份探针
  代码、同一场景顺序;`max=0` 的命中数见括号。每格是"该场景只跑一次"的冷启动值):

  | 模板 | `max` | v0.8.1 | v0.8.2 |
  | --- | --- | --- | --- |
  | 8x8(36000 命中) | 0 | 串行 571ms / parallel 184.9ms | **串行 ~9.9ms / parallel ~9.8ms** |
  | 8x8 | 1 | 串行 604µs / parallel 3.41ms | **~83µs / ~77µs**(稳态 ~0.7µs) |
  | 8x8(唯一性守卫) | 2 | 串行 442µs / parallel 3.10ms | **~7µs / ~7.5µs** |
  | 240x80(唯一性守卫) | 2 | 串行 104.4ms / parallel 553.2ms | **~147µs / ~147µs** |
  | 240x80(120 命中) | 0 | 串行 121.0s / parallel 35.3s | **~6.6ms / ~7.5ms** |

  稳态中位数见 `docs/GUIDE.md` 第 10 章的 criterion 交替对拍表(1080p:`max=0` 串行
  461ms → 8.2ms、parallel 138.2ms → 8.7ms;`max=1` 437.7µs → 0.64µs)。
- ⚠️ **两点代价,请按需评估**:① 有纹理画面的 `find_all` 慢一点(1080p criterion 交替 6 轮
  的中位:串行 3.99ms → 4.17ms,+4.5%;parallel 2.59ms → 2.82ms,+9%);② **平坦画面上
  `parallel` 不再比串行快**——
  重叠抑制必须严格按 `(y, x)` 顺序增量做,"被覆盖的列免验证"天生是顺序的,行块并行会把它
  打回逐候选验证(实测:240x80 模板 `max=0`,只做到块粒度早停的并行要 ~7.3s,而串行流式
  ~6.6ms)。所以这种画面下库里会主动放弃并行。

## v0.8.1 变更

修订号级别:没有破坏性改动(只新增一个方法),但有四处**行为修正**,升级后结果可能和 0.8.0 不同:

- **`find_all` 的返回顺序改成文档承诺的 `(y, x)` 升序**(从上到下、从左到右)。此前实际按
  `x` 优先排,且重叠去重"留先遇到的那个"因此留下的是**最左**而非**最上**的命中。现在与
  `find_color_on_screen` 口径一致。依赖旧顺序的代码需要复核。
- **`find_all` 的去重从二次方改为按行增量,`max` 现在真的限制工作量**。此前 `max` 只截断
  结果长度、扫描和去重照样跑完全程:1920x1200 纯色帧 + 8x8 纯色模板、`max=0`(36000 命中)
  实测 **23.5 秒**,`max=1` 也要 ~471ms。当时(v0.8.1)降到 **串行 ~571ms / 开 `parallel`
  ~185ms**(1920x1200 冷启动单次,与上面 v0.8.2 那表同一协议;这一节早期写的 ~347ms /
  ~151ms 在本机没能复现,已按复测值改掉),`max=1` ~0.6ms —— v0.8.2 又把它们压到
  **~9.9ms** 与 **~0.7µs**,见上面 v0.8.2 一节。
  `max` 给 0 的旧代码不会变错,只是从"卡住半小时"变成能跑完。
- **设了 `region` 且后端支持区域直抓(目前只有 `XCapCapture`)时,静态帧缓存开始生效**。
  此前这条路径把每帧都当"画面变了",于是"盯着一小块反复轮询"时缓存 100% 失效(实测静态
  画面连查 3 次搜 3 次,现在搜 1 次)。复用结果与重新搜一遍**完全一致**。`Gdi`/`Dxgi`/
  `Window` 走全屏路径,`changed` 由后端自己报,不受影响。
- **`find_until` / `wait_gone` 传 `Duration::MAX` 不再 panic**。此前 `Instant::now() + timeout`
  溢出会当场中止进程;现在算不出截止时刻就当永不超时。

新增:

- **`Match::center(&tpl) -> (i32, i32)`**:把命中换算成中心坐标。`find_center_on_screen`
  只覆盖单结果,`find_all` / `find_many` 的结果此前要自己写 `m.x + tpl.width as i32 / 2`。
- **`region` 完全落在画面外时提示一次**:`tracing` feature 开启后输出 `warn`(未改动返回值,
  仍是 `Ok(None)`),用来区分"region 坐标写错了"和"屏幕上确实没这张图"。
- **文档口径**:README/`Match` 字段注释都写明 `Rgb` 的 `score` 恒为 `1.0`(此前只有说明书
  里有);`MatchKind::Corr` 侧补上"ZNCC 走灰度、不认模板掩码"的提醒。

## v0.8 新功能

- **透明掩码模板**:`Template::load` 自动识别 PNG alpha;新增 `Template::from_rgba` / `with_mask`;`RgbMatcher` 比较时跳过掩码像素。
- **`find_center_on_screen`**:返回模板**中心**坐标,省去手动加半尺寸。
- **`diff_since_last(rect)`**:对比上一次截图,返回指定区域内颜色有变的像素数。
- **`CaptureKind::WindowByTitle(..)`**:`FinderBuilder` 直接按窗口标题精确匹配截窗口,无需先拿句柄。
- **图片格式扩展**:`Template::load` 现支持 **PNG + JPEG + WebP**(之前仅 PNG)。
- **GDI 后端不再 panic**:`BitBlt` / `GetDIBits` 失败(锁屏、休眠唤醒瞬间等)改为返回
  `Err`,与 DXGI / Window 后端行为一致。之前这里会 `assert!` 直接中止进程。
- **截图后端会自己跟上桌面尺寸变化**:GDI 每次抓取前比对桌面分辨率,变了就重建位图;DXGI
  在复制对象失效(改分辨率、锁屏 / 休眠唤醒、显卡重置)后自动重建并重取。之前 GDI 会一直
  输出错位画面、DXGI 会**永远停在失效前的旧帧**。
- **`find_all` 对带掩码的模板按可见区外接框去重**:此前按整张模板外接框判定,大模板 +
  小图标会把相邻目标漏报成一个。
- **`diff_since_last` 复用基线缓冲**,不再每次重新分配整帧:1080p 实测 5.6 ms → 3.4 ms。

## 升级到 v0.8 (breaking)
破坏面集中在 `Template` 的字段与两处行为变化:

| v0.7 | v0.8 |
| --- | --- |
| `Template` 只有 `rgb` / `width` / `height` 三个字段 | 多出 `pub mask: Option<Vec<bool>>`。用结构体字面量手工构造的代码需要补这个字段;走 `load` / `from_rgb` / `from_rgba` 构造的**不受影响** |
| `find_all` 按整张模板的外接框抑制重叠 | 带掩码的模板改按**可见像素外接框**抑制。同一张图上报的结果可能**变多**(小图标不再被大模板的外接框吞掉);无掩码时行为不变 |
| GDI 截图失败时 `assert!` panic | 返回 `Err`。原先靠捕获 panic 兜底的写法要改成处理 `Result` |
| 改分辨率 / 锁屏唤醒后需重建 `Finder` 才能恢复 | 后端自己跟上桌面尺寸变化,**无需重建** |

另一点:`CorrMatcher` 收到带掩码的模板时,debug 构建会 `debug_assert!` 提示,因为 ZNCC 那条路径
没有掩码接口、会给出与 `RgbMatcher` 不一致的结果。release 构建零开销,照常运行。

## 升级到 v0.7 (breaking)
`Error::Capture` 改成命名字段变体,让截图失败能保住**错误链**:

| v0.6 | v0.7 |
| --- | --- |
| `Error::Capture(String)`(`source()` 恒为 `None`) | `Error::Capture { message, source }` |
| 手写 `Error::Capture(msg.into())` | `Error::capture(msg)`;要带底层错误用 `Error::capture_from(ctx, e)` |

`message` 是"在哪一步失败"(如 `xcap Monitor::capture_image()`),`source` 是原始错误
(xcap 内部还会再包一层 io / D-Bus / Win32 错误,不保留就取不到了)。`Display` 仍把两者
拼成一行,所以只做 `e.to_string()` / `?` 上抛的代码**输出不变**,只有 `match` 到
`Error::Capture(s)` 的地方需要改成 `Error::Capture { message, .. }`。

## 升级到 v0.6 (breaking)
v0.6 把默认截图后端从已停维的 `screenshots` 换成 [xcap](https://crates.io/crates/xcap):

| v0.5 | v0.6 |
| --- | --- |
| `CaptureKind::Screenshots` | `CaptureKind::Monitor` |
| `ScreenshotsCapture` | `XCapCapture`(另有 `from_point()` 可绑指定显示器) |
| MSRV 1.75 | **MSRV 1.88**(下限来自 xcap 在 Linux 上的依赖 zbus;开 `match-corr` 需 1.89) |
| 限定区域 = 抓全屏再裁剪 | `Monitor` 后端**直接区域抓取**(端到端实测 47ms → 20ms) |

命中坐标语义不变:仍是**屏幕绝对坐标**。`Gdi` / `Dxgi` / `Window` 不支持区域直抓,
限定区域时自动回退为"抓全屏再裁剪",结果一致。

## 运行示例
```bash
# 默认(xcap / Monitor 后端)
cargo run --release --example find_on_screen -- path/to/template.png
# 用 Windows 快后端 + 自动选择
cargo run --release --features capture-dxgi,capture-gdi --example find_on_screen -- path/to/template.png
```

## 注意
- **分辨率 / DPI**:模板与截图需同一分辨率尺度,否则找不到。
- **文档**:`cargo doc --no-deps --open` 看完整 API 注释。**docs.rs 上本 crate 的构建目前是
  失败的**(`pixhunt-0.8.0` 在 docs.rs 的 builds 页显示 failed):原因是要编译依赖 `xcap`,
  而 `xcap` 的构建脚本需要系统库(libclang / PipeWire headers 等),docs.rs 环境没有这些包,
  不是本 crate 的代码问题。连带后果:在线文档里看不到 `CaptureKind::Gdi` / `Dxgi` /
  `Window` / `Auto` 这些 Windows 专有变体 —— 想查这些请直接看本地文档或源码。
- **截图 ≠ 匹配**:两者是分开计时/分开的步骤,别把截图耗时算进算法。
- **DXGI 限制**:RDP / 锁屏 / 无 GPU 时不可用,`CaptureKind::Auto` 会自动回退。
- **Linux 系统依赖**:默认后端 xcap 在 X11 走 xcb、Wayland 走 PipeWire/Wayland,
  编译需要:`pkg-config libclang-dev libxcb1-dev libxrandr-dev libdbus-1-dev
  libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev`(本仓库 CI 已按此配置)。
  缺失时报的是链接错误(如 `unable to find library -lgbm`),不是编译错误。
- **多显示器**:`Gdi` 只覆盖主显示器;`Dxgi` 取的是 `EnumAdapters(0)` + `EnumOutputs(0)`,
  单显卡时那就是主屏,多显卡(核显 + 独显)时**不保证**;`Monitor`(xcap)默认也取主屏。
  要抓副屏,用 `XCapCapture::from_point(x, y)`(按屏幕坐标落在哪块屏来选显示器),再交给
  `Finder::new(Box::new(cap), ...)`;或者用 `WindowCapture` 直接盯那个窗口。
- 帧字节序:GDI / DXGI / PrintWindow(`WindowCapture`)产出 **BGRA**,xcap 产出 **RGBA**;
  `RgbMatcher` 按帧的 `PixelFormat` 自动映射通道,无需你手动转换。自己拿像素做后续
  (存盘、送 OCR、转 `RgbaImage`)时这一步要看清,BGRA 当成 RGBA 用就是 R/B 整体互换。
- **多线程**:`Finder` 既不是 `Send` 也不是 `Sync`(它的字段是 `Box<dyn Capture>` /
  `Box<dyn Matcher>`,trait object 不会自动继承这两个标记),而且默认后端 `XCapCapture`
  本身也不是 `Send`(xcap 内部持有裸指针)。所以别想着把一个 `Finder` 传给线程或塞进
  `Arc`:要并发就在**每个线程里各自 `build()` 一个**(`Template` 是 `Send + Sync`,
  clone 进去即可),这种写法本机实测可编译。
- **平坦内容会让 `Rgb` 退化**:当模板与搜索区域**都**近乎单色(相邻像素几乎没有差异)
  时,锚点与逐像素早失败全部失效,扫描退化为暴力全量。1080p + 64px 实测:有纹理
  ~3ms,零方差模板放在纯色背景上 ~1.9s(慢 600 倍),且平坦区里会有多个"等价"命中点。
  `find_all` 在这种画面上还会返回**海量命中**(1920x1200 纯色帧 + 8x8 模板实测 36000 个,
  扫描本身现在只要 ~9ms,但这几万个 `Match` 既没意义又占内存),所以 `max` 请给真实上限,
  别习惯性写 0;只想确认"是不是只有一处"的话 `max = 2` 就够。
  同样,`find_on_screen` 在这种画面上给的 `Some` 只是**最上最左**的那个,不代表唯一。
  裁模板请选**有边缘、有纹理**的区域,别从纯色背景上切一块。

## 说明:内容由 AI 生成
本仓库的**代码、注释、测试与文档(含本 README)由 AI 编码助手生成或改写**,并经
`cargo test` / `cargo clippy -D warnings` / GitHub Actions 验证。仓库内的性能数字均为
特定机器上的测量值,只能当量级参考。内容**不保证逐行经过人工细读**,请按对待任何
第三方 crate 的方式自行评审后再用于生产。

## 许可
Licensed under either of **MIT** or **Apache License, Version 2.0** at your option.
See `LICENSE-MIT` and `LICENSE-APACHE`.
