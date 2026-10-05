# pixhunt 使用说明书(从零开始)

> 适用版本:**pixhunt 0.8.0**(2026-10-05 发布)。
> 读者假设:**你几乎没写过 Rust**,但你想在自己的屏幕上"找一张图在哪里",然后做点事。
> 这份说明书覆盖**全部对外 API**、每个参数的含义、能直接复制运行的示例、以及报错怎么排查。

> **说明**:本 crate 的代码、注释、测试与文档由 **AI 编码助手**生成或改写,并经 `cargo test` /
> `cargo clippy -D warnings` / CI 验证;不保证逐行经过人工细读,请按对待任何第三方依赖的方式自行评审。

---

## 目录

| 章节 | 内容 | 什么时候看 |
| --- | --- | --- |
| [第 1 章](#第-1-章这库能干什么白话版) | 这库能干什么(白话) | 完全不知道它在干嘛 |
| [第 2 章](#第-2-章准备工作装-rust建项目加依赖) | 准备工作:装 Rust、建项目、加依赖 | 还没环境 |
| [第 3 章](#第-3-章读懂-rust-代码的最小语法课) | 读懂 Rust 代码的最小语法课 | 看不懂 `&mut`、`?`、`Option` |
| [第 4 章](#第-4-章第一个程序逐行讲解) | 第一个程序(逐行讲解) | 想马上跑起来 |
| [第 5 章](#第-5-章核心概念白话版) | 核心概念:模板 / 帧 / 区域 / 坐标 / 像素格式 | 想明白为什么这么设计 |
| [第 6 章](#第-6-章功能开关features是什么) | 功能开关 features 是什么、怎么开 | 编译报"找不到某类型" |
| [第 7 章](#第-7-章api-总目录一览表) | **API 总目录**(一览表) | 想快速定位某个函数 |
| [第 8 章](#第-8-章逐个-api-详解) | **逐个 API 详解**(签名 + 参数 + 返回 + 坑) | 写代码时的参考手册 |
| [第 9 章](#第-9-章任务配方复制即用) | 任务配方:点击、等多个目标、等消失、副屏… | 有具体需求 |
| [第 10 章](#第-10-章性能与调参) | 性能与调参(实测数据) | 觉得慢 |
| [第 11 章](#第-11-章常见报错与排查) | 常见报错与排查 | 出错了 |
| [第 12 章](#第-12-章完整-api-速查表附录) | 完整 API 速查表(附录) | 打印出来贴墙上 |
| [第 13 章](#第-13-章术语表) | 术语表 | 遇到生词 |

---

## 第 1 章:这库能干什么(白话版)

想象你坐在电脑前,屏幕上有一堆窗口、按钮、图标。**程序看不见"意思"**,它只看得见一堆数字(每个像素的颜色值)。

pixhunt 做的事只有一件:

> **给你一张小图(叫"模板"),告诉你这张小图在当前屏幕上的哪个位置。**

返回的就是一个坐标,比如 `(842, 377)`——意思是"模板的**左上角**在屏幕第 842 列、第 377 行这个像素上"。

拿到坐标后,pixhunt 就退场了。**点击、输入、拖拽不归它管**(那需要另一个库,比如 `enigo`)。这是作者故意做的分工:

```
你的程序:pixhunt 找到按钮坐标  →  交给 enigo 在那个坐标点一下  →  界面响应
```

### 它能回答的具体问题

| 你想干什么 | 用哪个函数 | 在哪章 |
| --- | --- | --- |
| 全屏找一张按钮图,它在哪儿 | `Finder::find_on_screen` | 8.1 |
| 只在屏幕右下角这一块找(更快) | `Finder::builder().region(...)` / `set_region` | 8.1、8.2 |
| 屏幕上有好几个一样的图标,全找出来 | `Finder::find_all_on_screen` | 8.1 |
| 一次截图同时找 5 个不同的图 | `Finder::find_many_on_screen` | 8.1 |
| 等一个按钮出现(最多等 10 秒) | `Finder::find_until` | 8.1 |
| 等一个加载遮罩消失 | `Finder::wait_gone` | 8.1 |
| 找"红色像素连成的一坨"(血条、状态灯),没有图怎么办 | `Finder::find_color_on_screen` | 8.8 |
| 只截某个窗口(被别的东西挡住也能截) | `WindowCapture` | 8.5 |
| 图像被稍微调亮了一点也能找到 | `MatchKind::Rgb { tolerance }` | 8.2 |
| 光照变化明显、图有点变形也要找到 | `MatchKind::Corr`(ZNCC) | 8.7 |

### 它**不**是什么

- 不是 OCR(认字)。想识字要用专门的文字识别库。
- 不是"识别这是什么控件"。它只做**像素比对**,不懂语义。
- 不是 OpenCV 那种上千函数的图像库。它只做"截图 + 找位置",API 很小。
- 不认得"缩放到其他尺寸的模板"。模板必须和屏幕上的**一样大**。

---

## 第 2 章:准备工作(装 Rust、建项目、加依赖)

### 2.1 检查有没有 Rust

打开 PowerShell(按 `Win` 键,输入 `powershell`,回车),输入:

```powershell
cargo --version
```

- 如果显示类似 `cargo 1.88.0 (...)` → 已装好,跳到 2.2。
  **注意数字必须 ≥ 1.88**,pixhunt 的最低要求(MSRV)是 Rust 1.88。太旧就更新:
  ```powershell
  rustup update stable
  ```
- 如果提示"无法将 cargo 识别为 cmdlet…" → 没装。继续下一步。

### 2.2 安装 Rust(Windows)

1. 用浏览器打开 <https://rustup.rs>,下载 `rustup-init.exe`,双击运行。
2. 一路按回车选默认项(会装 stable 版 + cargo 构建工具)。
3. 装完**关掉 PowerShell 再重开一个**(环境变量要新窗口才生效),再跑一次 `cargo --version` 确认。

> macOS / Linux 同样在终端执行 `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`。
> Linux 还要额外装几个系统库,见第 6.4 节。

### 2.3 建一个新项目

```powershell
cd $HOME                    # 回到自己的目录
cargo new myhunter          # 新建一个叫 myhunter 的项目
cd myhunter
notepad .                   # 用记事本打开项目文件夹,看看结构
code .                      # 或者用 VS Code(装了 rust-analyzer 插件更好)
```

项目长这样:

```
myhunter/
├── Cargo.toml      ← 项目的"配置单",依赖写这里
└── src/
    └── main.rs     ← 你的代码写这里
```

### 2.4 把 pixhunt 加进依赖

用记事本打开 `Cargo.toml`,在 `[dependencies]` 下面加一行:

```toml
[dependencies]
pixhunt = "0.8"
```

想用 Windows 上更快的截图后端或更强的算法,就开对应"功能开关"(第 6 章详解):

```toml
[dependencies]
pixhunt = { version = "0.8", features = ["capture-dxgi", "capture-gdi", "capture-window", "match-corr", "parallel"] }
```

改完保存。**不需要手动下载**,第一次运行 `cargo run` 时 Cargo 会自动联网抓依赖。

> 也可以直接在命令行加,效果相同:
> ```powershell
> cargo add pixhunt --features capture-dxgi,capture-gdi
> ```

### 2.5 运行

```powershell
cargo run
```

第一次会编译依赖(几分钟),以后只需几秒。看到 `Hello, world!` 说明环境没问题。

---

## 第 3 章:读懂 Rust 代码的最小语法课

说明书里所有示例都会遇到这 7 个符号。看懂这一章,后面的代码就不会卡住。

| 写法 | 白话意思 | 为什么需要 |
| --- | --- | --- |
| `let x = 5;` | 定义一个变量 `x` | `let` 是"我要开始用一个变量"的意思 |
| `let mut f = ...;` | 定义一个**可以改**的变量 `f` | Rust 默认变量不可变。`Finder` 要反复截图,内部状态会变,所以**必须加 `mut`**,否则编译报错 |
| `&tpl` | "把 tpl **借**给我看看,不还也不用负责" | 加 `&` 表示只借用不拿走。函数参数里看到 `&Template` 就这么读 |
| `&mut f` | "借给你,而且**允许你改**" | `finder.find_on_screen(&tpl)` 里的 `finder` 自己是 `&mut self`,所以调用它的变量必须声明成 `mut` |
| `Option<Match>` | "**可能有,可能没有**" | 屏幕上找不到按钮是常事。它要么装着一个 `Match`(写作 `Some(m)`),要么什么都没(`None`) |
| `Result<T, Error>` | "**可能成功给出 T,可能出错给出 Error**" | 截图失败(没显示器、权限被拒)是真实存在的,必须处理 |
| `?` | "如果出错就**直接把错误往上抛**,正常才继续" | `finder.find_on_screen(&tpl)?` 读作:成功了就拿到值;失败了就立刻从当前函数返回那个错误 |

### 3.1 怎么把 `Option` 里的值取出来(最常用的四种写法)

```rust
// 假设 m 的类型是 Option<Match>
if let Some(m) = opt {
    println!("找到了,在 ({}, {})", m.x, m.y);   // 这里 m 才是真正的 Match
}

if opt.is_some() { /* 找到了 */ }
if opt.is_none() { /* 没找到 */ }

let inner = opt.unwrap();          // 强行取出;如果是 None 程序会当场崩溃 → 只适合写测试/一次性脚本
let inner = opt.expect("这里必须有");  // 同上,崩溃时多打印你写的话

let fallback = opt.unwrap_or(Match { x: 0, y: 0, score: 0.0 });  // 没有就用个默认值
```

### 3.2 怎么把 `Result` 交给调用者

`main` 函数也可以返回 `Result`,这样你就能在里面放心地写 `?`:

```rust
fn main() -> pixhunt::Result<()> {
    // ... 你的代码,任何一步失败都会自动打印错误并让程序以失败码退出
    Ok(())      // 结尾必须给出 Ok(()) 表示"main 正常结束"
}
```

> `<()>` 是个空元组,Rust 规定 main 必须返回点什么东西,`()` 就是"没实际内容"的意思。
> 你照抄 `pixhunt::Result<()>` 和最后的 `Ok(())` 就行。

### 3.3 结构化绑定(看别人代码会遇到)

```rust
let m = finder.find_on_screen(&tpl)?;        // m: Option<Match>
if let Some(hit) = m {
    println!("{}", hit.x);
}
```

`if let Some(x) = 值 { ... }` 就是"**如果有,就把它取名叫 x 并用起来**"。

---

## 第 4 章:第一个程序(逐行讲解)

目标:**在屏幕上找一张图,找到就把坐标打印出来。**

### 4.1 先准备一张模板图

1. 用 Windows 自带的**截图工具**(搜索"截图工具"或按 `Win+Shift+S`),截下你想找的那个小图,**只截它本身**,别带周围背景。
2. 保存成 PNG,比如 `D:\pic\button.png`。
   - 尺寸建议 **20~200 像素见方**。太小(比如 4x4)容易在屏幕上撞到假的;太大(比如整屏)没必要。
   - **必须是 PNG**,当前版本只解码 PNG(见 11.4)。
   - 截图时**不要缩放**。缩放会让像素和屏幕上不一样,导致找不到。

### 4.2 代码

把 `src/main.rs` 整个替换成:

```rust
use pixhunt::{CaptureKind, Finder, MatchKind, Template};   // 1
use std::time::Duration;                                    // 2

fn main() -> pixhunt::Result<()> {                          // 3
    let tpl = Template::load("D:/pic/button.png")?;         // 4
    println!("模板尺寸 {}x{}", tpl.width, tpl.height);       // 5

    let mut finder = Finder::builder()                      // 6
        .capture(CaptureKind::Monitor)                      // 7
        .matcher(MatchKind::Rgb { tolerance: 25 })          // 8
        .build()?;                                          // 9

    match finder.find_on_screen(&tpl)? {                    // 10
        Some(m) => println!("命中!左上角在 ({}, {}),相似度 {}", m.x, m.y, m.score),
        None => println!("屏幕上没找到这张图"),
    }

    // 11:等它出现,最多 10 秒,每 100 毫秒看一次
    let m = finder.find_until(&tpl, Duration::from_secs(10), Duration::from_millis(100))?;
    println!("等待结果: {:?}", m.map(|m| (m.x, m.y)));

    Ok(())                                                  // 12
}
```

### 4.3 逐行解释

| 行 | 解释 |
| --- | --- |
| 1 | 把要用的四个类型从库里"引进来"。少写一行就会报"找不到 `Finder`" |
| 2 | `Duration` 是标准库的"时长"类型,用来表达 10 秒、100 毫秒 |
| 3 | main 返回 `pixhunt::Result<()>`,于是里面能用 `?` 偷懒 |
| 4 | 读图片文件成模板。`?`:读不到文件(路径错、不是 PNG)就把错误抛出去 |
| 5 | 模板的宽和高是可访问的公开字段。这一行能帮你确认"图确实读对了" |
| 6 | 开始搭一个 `Finder`(找图器)。注意 `let mut`:第 10 行要反复用它截图,内部会变 |
| 7 | 选截图后端。`Monitor` = 跨平台保底(xcap),不开任何 feature 也能用 |
| 8 | 选匹配算法。`Rgb { tolerance: 25 }` = 极速 RGB 比对,每通道允许 25 的误差 |
| 9 | `build()` 可能失败(比如指定了 `Dxgi` 但这机器用不了),所以有 `Result`,要 `?` |
| 10 | **截一屏 + 找图**。返回 `Option<Match>`:`Some` 是找到,`None` 是没有 |
| 11 | 轮询等待。命中立刻返回;超时返回 `None`。`.map(...)` 只是把结果里的坐标抽出来好看一点 |
| 12 | 告诉 main"我正常跑完了" |

### 4.4 跑起来

```powershell
cargo run
```

典型输出:

```
模板尺寸 64x24
命中!左上角在 (1024, 560),相似度 1
等待结果: Some((1024, 560))
```

跑不出来的话,先去看[第 11 章报错排查](#第-11-章常见报错与排查),八成是这几个原因:模板是从别台机器/别的缩放比例截的、`tolerance` 太小、或者模板图本身是一片纯色。

---

## 第 5 章:核心概念(白话版)

### 5.1 模板(Template):你要找的那张小图

- 类型:`pixhunt::Template`。
- 内容:一张已经解码的图,每个像素 **3 个字节,固定 RGB 顺序**(没有透明度通道)。
- 它**永远不变**:一次 `load` 之后只读。所以同一个模板可以反复找、找很多次,库内部还能把它算好的东西缓存起来。

### 5.2 帧(Frame):一屏画面的原始数据

- 类型:`pixhunt::Frame`。
- 内容:一大串字节 `pixels` + 宽 `width` + 高 `height` + 通道顺序 `format`。
- **每像素 4 个字节**,一行紧跟一行、没有行末填充(所以 `stride = width * 4`)。
- `format` 可能是 `Rgba8`(R,G,B,Alpha)或 `Bgra8`(B,G,R,Alpha)。

为什么要区分?因为不同的截图方式给的顺序不一样:

| 后端 | 像素顺序 |
| --- | --- |
| `XCapCapture`(Monitor) | RGBA |
| `GdiCapture` / `DxgiCapture` / `WindowCapture` | BGRA |

pixhunt 在比较像素时**自动按帧的格式去取对应的字节**,所以你不用自己转、也不会因为换了后端就找不到图。这一点很重要:**模板恒为 RGB,帧可能是 BGRA,库内部自己映射。**

### 5.3 坐标:左上角是原点,x 向右,y 向下

```
(0,0) ──────────────────► x
  │
  ▼   屏幕 1920 x 1200
  y
```

- `Match.x` / `Match.y` 是**模板左上角**在屏幕上的位置,**不是中心**。
- 类型是 `i32`(带符号整数,因为窗口相对截图等场景可能出现负数)。
- 想点子中心(用来点击):**中心 = 左上角 + 模板尺寸的一半**:
  ```rust
  let cx = m.x + tpl.width as i32 / 2;
  let cy = m.y + tpl.height as i32 / 2;
  ```
- 全屏后端(`Monitor`/`Gdi`/`Dxgi`)返回的是**屏幕绝对坐标**;只有 `WindowCapture` 返回的是**相对该窗口客户区左上角**的坐标(见 8.5.5)。

### 5.4 区域(Rect):只在屏幕上圈一块来找

- 类型:`pixhunt::Rect`,四个字段 `x, y, width, height`,都是 `usize`(非负整数)。
- 用途:你明知按钮只在右下角,就别让库扫整屏——**限定区域是最有效的提速手段**(实测端到端 47ms → 20ms)。
- 写法有两种,等价:
  ```rust
  use pixhunt::Rect;
  .region(Rect::new(1200, 800, 700, 400))   // 明确写法
  .region((1200, 800, 700, 400))            // 元组自动转成 Rect
  ```
- 区域会被**自动夹紧**到屏幕内(超出部分丢掉),所以给个"肯定够大"的矩形不会崩。
- ⚠️ `Rect` 用的是 `usize`,**表达不了负坐标**。所以副屏(原点带负偏移)上的区域目前只能回退成"截全屏再裁剪"。

### 5.5 "插座":后端与算法可以各自换

pixhunt 故意把两件事分开:

```
       Capture(怎么拿到画面)          Matcher(怎么找图)
             │                            │
             └────►   Finder   ◄──────────┘
                        │
                        └────► find_on_screen / find_until / ...
```

- 换截图方式 = 换 `Capture` 实现,匹配逻辑一点不用动。
- 换算法 = 换 `Matcher` 实现,截图一点不用动。
- 日常你只用 `Finder::builder()` 挑现成的;想自己写一个后端,实现 `Capture` trait 就行(8.5)。

### 5.6 相似度 score

| 匹配器 | score 的取值 | 含义 |
| --- | --- | --- |
| `Rgb` | **永远是 1.0** | 它是"通过/不通过"判定:容差内逐像素全对才算命中,不存在"90% 像" |
| `Corr`(ZNCC) | `0.0 ~ 1.0` | 归一化互相关,**越大越像**。1.0 = 完美,0.7 已算很强,0.3 基本是噪声 |

所以别用 `Rgb` 的 score 去筛"最像的那个"——它只有 1.0。要分数就用 `Corr`。

---

## 第 6 章:功能开关(features)是什么

### 6.1 白话解释

Rust 的 `feature` 就是**编译时的可选开关**。库里写了很多代码,但默认只编译一小部分;你要别的功能,就显式打开,附带把对应的依赖也拉进来。

好处:默认安装最快、依赖最少。坏处:**忘了开就报"找不到这个类型/这个变体"**,新手最容易在这卡住。

### 6.2 pixhunt 的全部开关

| feature | 打开后多出来的东西 | 平台 |
| --- | --- | --- |
| (不开任何) | `XCapCapture`、`CaptureKind::Monitor`、`RgbMatcher`、颜色搜索 | 全平台 |
| `capture-gdi` | `GdiCapture`、`CaptureKind::Gdi`、`CaptureKind::Auto` | 仅 Windows |
| `capture-dxgi` | `DxgiCapture`、`CaptureKind::Dxgi`、`CaptureKind::Auto` | 仅 Windows |
| `capture-window` | `WindowCapture`、`WindowHandle`、`CaptureKind::Window(..)`、`CaptureKind::WindowByTitle(..)` | 仅 Windows |
| `match-corr` | `CorrMatcher`、`CorrConfig`、`MatchKind::Corr` / `CorrWith(..)` | 全平台(需 Rust **1.89**) |
| `parallel` | `RgbMatcher` 按行并行、ZNCC 分层并行(结果完全一致) | 全平台 |
| `tracing` | 库内部输出 trace 级诊断(截图耗时、缓存跳过、命中与否) | 全平台 |

注意几个坑:

- 只在 Windows 上有效的 feature,在 Linux/macOS 上打开**不会报错但也不会生效**(代码被 `#[cfg(windows)]` 关掉了)。
- `CaptureKind::Auto` 需要 `capture-gdi` 或 `capture-dxgi` **至少一个**打开才存在。
- `parallel` 会通过一种叫 weak dep 的机制把并行能力**传导给 `corrmatch`**;如果你开了 `match-corr` 却没开 `parallel`,`CorrConfig.parallel` 会被自动归为 `false`(不然会更糟:静默找不到)。
- 开 `match-corr` 后编译下限变成 **Rust 1.89**(它的依赖 `wide`/`safe_arch` 要求),而默认功能是 1.88。

### 6.3 怎么写 Cargo.toml / 命令行临时开

```toml
# 永久:写进 Cargo.toml
[dependencies]
pixhunt = { version = "0.8", features = ["capture-dxgi", "match-corr", "parallel"] }
```

```powershell
# 临时:只对这一次编译/运行打开
cargo run --features capture-dxgi
cargo test --all-features        # 全部开关都打开(库作者自测就这么跑)
```

### 6.4 Linux 还需要系统库(重要)

默认后端 `XCapCapture` 依赖 `xcap`,它在 Linux 上要链接 X11(XCB/Xrandr)、D-Bus、PipeWire、Wayland、EGL、GBM 的系统开发包。Debian/Ubuntu:

```bash
sudo apt-get install -y --no-install-recommends \
  pkg-config libclang-dev \
  libxcb1-dev libxrandr-dev libdbus-1-dev \
  libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev
```

缺包时报的是**链接错误**(如 `rust-lld: error: unable to find library -lgbm`),不是"代码错误",新手很容易误会是库的 bug。

---

## 第 7 章:API 总目录(一览表)

库里能被你调用的东西**总共就这些**。`〔f:xxx〕` 表示需要先开那个 feature。

### 7.1 类型总览

| 类型 | 一句话职责 | 要不要开 feature | 详解 |
| --- | --- | --- | --- |
| `Finder` | 组装好的"找图器",你 95% 的时间只跟它打交道 | 不要 | 8.1 |
| `FinderBuilder` | `Finder` 的装配线(`Finder::builder()` 得到它) | 不要 | 8.2 |
| `CaptureKind` | 选哪种截图后端 | 部分要 | 8.2 |
| `MatchKind` | 选哪种匹配算法 | `Corr` 要 `match-corr` | 8.2 |
| `Template` | 你要找的模板小图 | 不要 | 8.4 |
| `Match` | 一次命中的结果 `(x, y, score)` | 不要 | 8.4 |
| `Frame` | 一帧画面(像素 + 尺寸 + 通道顺序) | 不要 | 8.3 |
| `Rect` | 一个矩形区域(限定查找范围用) | 不要 | 8.3 |
| `PixelFormat` | 帧的通道顺序:`Rgba8` / `Bgra8` | 不要 | 8.3 |
| `Capture`(trait) | "怎么拿到画面"的插座 | 不要 | 8.5 |
| `XCapCapture` | 跨平台后端(xcap) | 不要 | 8.5 |
| `GdiCapture` | Windows GDI 后端 | 〔f:capture-gdi〕 | 8.5 |
| `DxgiCapture` | Windows DXGI 后端(最快) | 〔f:capture-dxgi〕 | 8.5 |
| `WindowCapture` | Windows 截单个窗口(PrintWindow) | 〔f:capture-window〕 | 8.5 |
| `WindowHandle` | 窗口句柄(其实就是 `isize`) | 〔f:capture-window〕 | 8.5 |
| `Matcher`(trait) | "怎么找模板"的插座 | 不要 | 8.6 |
| `RgbMatcher` | 极速 RGB 逐像素比对(内置) | 不要 | 8.6 |
| `CorrMatcher` | ZNCC 高鲁棒匹配(灰度 + 金字塔) | 〔f:match-corr〕 | 8.7 |
| `CorrConfig` | ZNCC 的调参面板 | 〔f:match-corr〕 | 8.7 |
| `ColorSpec` | 要找的颜色 + 容差 | 不要 | 8.8 |
| `ColorBlob` | 找到的一个连通色块 | 不要 | 8.8 |
| `FindColor`(trait) | 给 `Frame` 加 `.find_blobs(..)` 方法 | 不要 | 8.8 |
| `Error` / `Result` | 统一错误类型与 `Result` 别名 | 不要 | 8.9 |

模块路径:`Finder`、`Frame`、`Template` 等都在 crate 根(`use pixhunt::Finder;`)。
少数只在子模块里:`pixhunt::finder::FinderBuilder`、`pixhunt::color::find_blobs`。

### 7.2 方法总览(按"我要做什么"排)

| 我想… | 调用 | 返回 |
| --- | --- | --- |
| 建一个找图器 | `Finder::builder().capture(..).matcher(..).region(..).build()?` | `Result<Finder>` |
| 全屏找一次 | `finder.find_on_screen(&tpl)?` | `Result<Option<Match>>` |
| 全屏找出所有(不重叠) | `finder.find_all_on_screen(&tpl, max)?` | `Result<Vec<Match>>` |
| 一次截图找多个不同模板 | `finder.find_many_on_screen(&[&a, &b])?` | `Result<Vec<Option<Match>>>` |
| 等它出现 | `finder.find_until(&tpl, timeout, interval)?` | `Result<Option<Match>>` |
| 等它消失 | `finder.wait_gone(&tpl, timeout, interval)?` | `Result<bool>` |
| 按颜色找色块 | `finder.find_color_on_screen(&spec, min_area)?` | `Result<Vec<ColorBlob>>` |
| 不截图,在已有帧里找 | `finder.find_in_frame(&frame, &tpl)` | `Option<Match>` |
| 找到后直接拿中心坐标(免手动加半尺寸) | `finder.find_center_on_screen(&tpl)?` | `Result<Option<Match>>` |
| 判断"这块区域变了没" | `finder.diff_since_last(rect)?` | `Result<u32>` |
| 改限定区域 | `finder.set_region(Some(Rect::new(..)))` / `set_region(None)` | `()` |
| 读区域 | `finder.region()` | `Option<Rect>` |
| 单独截一帧 | `capture.grab()?` / `capture.grab_into(&mut dst)?` | `Result<Frame>` / `Result<bool>` |
| 试区域直抓 | `capture.grab_region(rect)?` | `Result<Option<Frame>>` |
| 问后端叫什么名字 | `capture.backend()` | `&'static str` |
| 只用匹配器在帧里找 | `matcher.find(..)` / `find_in(..)` / `find_all(..)` | 同上 |
| 读模板文件 | `Template::load("a.png")?` | `Result<Template>` |
| 内存里造模板 | `Template::from_rgb(rgb, w, h)` | `Template` |
| 裁一小块帧 | `frame.crop(rect)` | `Frame` |
| 帧转灰度 | `frame.to_gray()` / `frame.to_gray_into(&mut buf)` | `Vec<u8>` / `()` |

---

## 第 8 章:逐个 API 详解

格式说明:先给**源码里的真实签名**,再讲每个参数、返回值、什么时候用、以及坑。

### 8.1 `Finder` —— 你主要用的那个类型

```rust
pub struct Finder { /* 私有字段 */ }
```

它是"一个截图后端 + 一个匹配器 + 可选区域"的组合。内部**复用一个 `Frame` 缓冲**,
所以连续查找不会每帧重新分配内存——这也是为什么它的方法大多要求 `&mut self`
(变量要声明成 `let mut finder`)。

#### 8.1.1 `Finder::builder() -> FinderBuilder`

最省心的创建方式。不设置任何东西也能用,默认值见 8.2.4。

```rust
let mut finder = Finder::builder().build()?;   // 全默认:Monitor 后端 + Rgb{tolerance:25} + 整屏
```

#### 8.1.2 `Finder::new(capture: Box<dyn Capture>, matcher: Box<dyn Matcher>) -> Self`

**逃生口**:你自己造好后端和匹配器(比如实现了 `Capture` trait,或要用
`XCapCapture::from_point(..)` 绑副屏)时用它。注意它**不返回 `Result`**——
构造本身不会失败,失败发生在截图的时候。

```rust
use pixhunt::{Capture, Finder, Matcher, RgbMatcher, Template, XCapCapture};

// 绑到坐标 (2560, 0) 所在的那台显示器(副屏)
let cap = XCapCapture::from_point(2560, 0)?;
let m: Box<dyn Matcher> = Box::new(RgbMatcher::new(25));
let mut finder = Finder::new(Box::new(cap), m);
let hit = finder.find_on_screen(&Template::load("btn.png")?)?;
```

`Box<dyn Capture>` 这种写法的意思:"把一个实现了 `Capture` 的东西装进盒子"。
照抄 `Box::new(你的后端)` 即可。

#### 8.1.3 `find_on_screen(&mut self, tpl: &Template) -> Result<Option<Match>>`

**截一屏 → 在里面找模板 → 返回结果。** 如果设了 `region`,只在该区域内找。

- 返回 `Ok(Some(m))`:找到,`m.x/m.y` 是**屏幕绝对坐标**(模板左上角)。
- 返回 `Ok(None)`:屏幕上没有(这不是错误!别用 `unwrap`)。
- 返回 `Err(...)`:截图本身失败了(后端不可用、显示器消失等)。

⚠️ 每次调用都会**真的截一屏**。在循环里连刷要用 `find_until`(它有缓存优化)。

**缓存跳过机制**:如果后端告诉你"画面自上次以来没变"(目前只有 `DxgiCapture` 会这么报,
`Monitor`/`Gdi`/`Window` 都保守地报"已变"),且模板和区域都和上次一样,
那就直接复用上次结果、跳过搜索。结果与重新搜一遍**完全一致**,你可以当它不存在。

#### 8.1.4 `find_all_on_screen(&mut self, tpl: &Template, max: usize) -> Result<Vec<Match>>`

截一屏,找出**全部互不重叠**的命中,按 `(y, x)` 升序(从上到下、从左到右)。

- `max`:最多要几个。`max = 0` 表示**不限**。
- "不重叠"的定义:两个命中如果在 x 方向相差小于模板宽度**且** y 方向相差小于模板高度,
  就算同一个目标,只留先遇到的那个。所以同一个图标挨着排开不会返回一堆半重叠的假命中。
- ⚠️ **用 `CorrMatcher` 时它只返回 1 个**。原因:`find_all` 在 `Matcher` trait 里有个基于
  裁剪的默认实现,只转发单个 `find`;`RgbMatcher` 覆写了它,`CorrMatcher` 没有(见 8.6.1)。
  要"多目标 + 抗光照"目前得自己移动 `region` 分次找。

```rust
let all = finder.find_all_on_screen(&tpl, 0)?;
for (i, m) in all.iter().enumerate() {
    println!("第 {} 个 @ ({}, {})", i + 1, m.x, m.y);
}
```

#### 8.1.5 `find_many_on_screen(&mut self, tpls: &[&Template]) -> Result<Vec<Option<Match>>>`

**只截一屏**,然后依次匹配每个模板。返回的 `Vec` **与传入顺序一一对应**,没找到的位置是 `None`。

用途:一次判断"当前是哪个界面"(5 个界面的标志性按钮各截一张)。比调用 5 次
`find_on_screen` 省下 4 次截图(实测一次全屏截图 16~34ms,是最大开销项)。

```rust
let a = Template::load("home.png")?;
let b = Template::load("login.png")?;
let rs = finder.find_many_on_screen(&[&a, &b])?;
if rs[0].is_some() { println!("当前在主页"); }
```

#### 8.1.6 `find_until(&mut self, tpl: &Template, timeout: Duration, interval: Duration) -> Result<Option<Match>>`

轮询等目标**出现**:每 `interval` 查一次,命中立即返回;超过 `timeout` 返回 `Ok(None)`。

- 命中不算超时:哪怕只剩 1 毫秒,查到就返回 `Some`。
- `interval` 不会被睡过头:最后一轮会取 `min(interval, 剩余时间)`。
- 等待期间每轮都是真截图。想省 CPU:用 `Dxgi` 后端(静态画面会被跳过),或把 `interval` 设大点。

```rust
use std::time::Duration;
let m = finder.find_until(&tpl, Duration::from_secs(10), Duration::from_millis(80))?;
```

#### 8.1.7 `wait_gone(&mut self, tpl: &Template, timeout: Duration, interval: Duration) -> Result<bool>`

轮询等目标**消失**。

- `Ok(true)`:在 `timeout` 内确实找不到了(等待成功)。
- `Ok(false)`:到时间了还在(比如加载遮罩一直转)。
- 语义与 `find_until` 相反,注意别搞混:**它返回 `bool` 而不是 `Option<Match>`**。

典型用途:等"正在保存…"的提示条不见了再继续下一步。

```rust
let gone = finder.wait_gone(&loading_mask, Duration::from_secs(30), Duration::from_millis(200))?;
if !gone { println!("30 秒了还在转,可能卡住了"); }
```

#### 8.1.8 `find_color_on_screen(&mut self, spec: &ColorSpec, min_area: usize) -> Result<Vec<ColorBlob>>`

**不需要模板图**,按颜色范围找画面上的连通色块(血条、状态灯、高亮框)。详见 8.8。

- `spec`:目标颜色 + 每通道容差。
- `min_area`:过滤碎点,匹配像素数小于它的色块被丢掉。想滤掉抗锯齿边缘毛刺,给 8~30。
- 返回按 `(bounds.y, bounds.x)` 升序;设了 `region` 时坐标也已换算成屏幕绝对坐标。

#### 8.1.9 `find_in_frame(&self, frame: &Frame, tpl: &Template) -> Option<Match>`

**不截图**,直接在给定的帧里找。遵循当前 `region`(有区域就走 `find_in`,没有就走 `find`)。

用途:截一次图存下来,拿它匹配几十个模板;或做单元测试(用假帧,不依赖真屏幕);
或离线分析别处拿到的原始像素数据。

注意它是 `&self`(**不需要 `mut`**),这在 `Finder` 的方法里是少见的。

```rust
let frame = capture.grab()?;                    // 自己截图
let hit = finder.find_in_frame(&frame, &tpl);   // 之后匹配多少个模板都不用再截
```

#### 8.1.10 `set_region(&mut self, region: Option<Rect>)` 与 `region(&self) -> Option<Rect>`

运行中改 / 读限定区域。`None` = 整屏。

`set_region` 会**作废内部的结果缓存**(否则换个区域还返回上一个区域的旧结果就错了)。

```rust
finder.set_region(Some(Rect::new(1200, 800, 700, 400)));  // 只看右下角一块
finder.set_region(None);                                   // 恢复全屏
let cur = finder.region();                                 // 读回来
```

#### 8.1.11 `find_center_on_screen(&mut self, tpl: &Template) -> Result<Option<Match>>`

与 `find_on_screen` 相同,但返回的 `x`/`y` 是模板**中心**而非左上角(省去手动加半尺寸)。

```rust
if let Some(m) = finder.find_center_on_screen(&tpl)? {
    // m.x, m.y 就是可以直接点击的中心坐标
    println!("点击 ({}, {})", m.x, m.y);
}
```

#### 8.1.12 `diff_since_last(&mut self, rect: Rect) -> Result<u32>`

截一屏后与**上一次截图**对比,返回 `rect` 内颜色有差异的像素数量。

- 首次调用(无基线)返回 `rect.width * rect.height`(视为"全部变了")。
- 比较 R/G/B 三通道(忽略 Alpha),任一通道不等就算"不同"。
- 调用后当前帧成为下一次比较的基线。
- 用途:判断"这块区域动画停了没""有没有新消息红点亮起"。

```rust
let changed = finder.diff_since_last(Rect::new(100, 100, 200, 50))?;
if changed == 0 {
    println!("画面静止");
} else {
    println!("有 {} 个像素变了", changed);
}
```

### 8.2 装配:`FinderBuilder`、`CaptureKind`、`MatchKind`

#### 8.2.1 `FinderBuilder` 的四个方法

```rust
pub fn capture(self, k: CaptureKind) -> Self
pub fn matcher(self, k: MatchKind) -> Self
pub fn region(self, r: impl Into<Rect>) -> Self
pub fn build(self) -> Result<Finder>
```

前三个是"链式设置":吃掉 `self` 再吐回来,所以能一行接一行写。
`impl Into<Rect>` 的意思是:**`Rect` 或 `(x, y, width, height)` 元组都行**。

`build()` 返回 `Result`。目前只有两种失败:选 `Dxgi` 但这台机器开不了桌面复制
(报 `capture error: DXGI desktop duplication unavailable`),或选 `Monitor` 却拿不到显示器
(报 `capture error: xcap Monitor::all(): ...`)。

⚠️ `FinderBuilder` **没有**从 crate 根重新导出。你不需要给它命名(链式调用即可);
真要在函数签名里写它,用 `pixhunt::finder::FinderBuilder`。

#### 8.2.2 `CaptureKind`(选后端)

```rust
pub enum CaptureKind {
    Monitor,                          // 跨平台保底(xcap),输出 RGBA
    Gdi,                              // 〔f:capture-gdi〕   Windows,输出 BGRA
    Dxgi,                             // 〔f:capture-dxgi〕  Windows,最快,输出 BGRA
    Window(WindowHandle),             // 〔f:capture-window〕截指定窗口,坐标为窗口相对
    WindowByTitle(String),            // 〔f:capture-window〕按标题精确匹配截窗口
    Auto,                             // 〔f:capture-gdi 或 capture-dxgi〕依次试 DXGI → GDI → xcap
}
```

实测怎么选(1920x1200,release 构建,中位数):

| 选项 | `backend()` 返回 | 全屏截图 | 什么时候选它 |
| --- | --- | --- | --- |
| `Dxgi` | `"dxgi"` | **~16ms** | Windows 首选;有"画面没变就跳过"的加成 |
| `Monitor` | `"xcap"` | ~34ms | 要跨平台、或要**区域直抓**(见 8.5.2) |
| `Gdi` | `"gdi"` | ~32ms | DXGI 不可用时的 Windows 保底 |
| `Auto` | 取决于选中谁 | 16~34ms | 懒得想就它;RDP/锁屏会自动退级 |
| `Window(h)` | `"print-window"` | 取决于窗口大小 | 只盯某个窗口、或它被挡住了 |
| `WindowByTitle("...")` | `"print-window"` | 取决于窗口大小 | 同上,但免手动拿句柄(标题精确匹配) |

两个要命的限制:

- `Dxgi` 在**远程桌面(RDP)、锁屏界面、没有 GPU** 时不可用 → `build()` 直接报错。
- `Monitor` / `Gdi` / `Dxgi` **只覆盖主显示器**。要副屏:用 `XCapCapture::from_point(x, y)`
  再 `Finder::new(..)`(8.1.2),或 `CaptureKind::Window(句柄)`。

#### 8.2.3 `MatchKind`(选算法)

```rust
pub enum MatchKind {
    Rgb { tolerance: i32 },     // 极速 RGB 比对(内置)
    Corr,                       // 〔f:match-corr〕 ZNCC,抗光照/轻微缩放
    CorrWith(CorrConfig),       // 〔f:match-corr〕 自定义 ZNCC 参数
}
```

`tolerance` 怎么填(每通道允许的最大绝对差,0~255):

| tolerance | 含义 | 适合 |
| --- | --- | --- |
| `0` | 逐像素完全相等 | 图标绝对不变、同一台机器同一缩放 |
| `10~20` | 轻微差异 | 抗锯齿、深浅主题的细微不同 |
| **`25`(默认)** | 约等于"亮度差 10%" | 日常最稳的起点 |
| `40+` | 很宽松 | 容易撞到"差不多"的假命中,慎用 |

选 `Rgb` 还是 `Corr`:

- **默认用 `Rgb`**。1080p 全屏纯匹配串行 ~3.0ms(开 `parallel` ~1.7ms)。
- 屏幕会变亮变暗、模板是拍的照片、有轻微缩放 → 用 `Corr`。代价:串行 ~13.7ms、并行 ~6.6ms,
  换来的是 `score` 真的是相似度分数。
- ⚠️ `Corr` 工作在**灰度**上:纯色/零方差模板(比如一整块纯红)在它眼里"没有对比度",
  匹配不到属预期。这种场景该用颜色搜索(8.8)。

#### 8.2.4 默认值(不用猜)

| 设置项 | 不写时的默认 |
| --- | --- |
| `.capture(..)` | `CaptureKind::Monitor` |
| `.matcher(..)` | `MatchKind::Rgb { tolerance: 25 }` |
| `.region(..)` | `None`(整屏) |

### 8.3 `Frame`、`Rect`、`PixelFormat`

#### 8.3.1 `PixelFormat`

```rust
pub enum PixelFormat { Rgba8, Bgra8 }
```

每像素 4 字节中前三个通道的顺序。第四个字节 Alpha 一律被忽略。

#### 8.3.2 `Rect`

```rust
pub struct Rect { pub x: usize, pub y: usize, pub width: usize, pub height: usize }
impl Rect { pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self }
impl From<(usize, usize, usize, usize)> for Rect   // 元组自动转 Rect
```

- 坐标**左上原点**,单位是像素。
- 字段是 `usize`(非负)⇒ **不能表示负坐标**。副屏原点带负偏移、跨屏区域,只能走全屏路径。
- `new` 是 `const fn`,可以当常量用。

#### 8.3.3 `Frame` 的公开字段

```rust
pub struct Frame {
    pub pixels: Vec<u8>,   // 每像素 4 字节,行连续无填充
    pub width: usize,
    pub height: usize,
    pub format: PixelFormat,
}
```

想自己读某个像素的正确写法:

```rust
let (ro, go, bo) = frame.rgb_offsets();           // 关键:按格式取偏移,别硬写 0/1/2
let i = (y * frame.width + x) * 4;
let (r, g, b) = (frame.pixels[i + ro], frame.pixels[i + go], frame.pixels[i + bo]);
```

#### 8.3.4 `Frame` 的方法

| 签名 | 用途 | 注意 |
| --- | --- | --- |
| `rgba8(width, height, pixels) -> Frame` | 用 RGBA 字节造一帧 | 不校验长度,自己保证 `pixels.len() == w*h*4` |
| `bgra8(width, height, pixels) -> Frame` | 用 BGRA 字节造一帧 | 同上 |
| `full_rect(&self) -> Rect` | 整帧大小的 `Rect` | 传给 `find_all` / `find_blobs` 很方便 |
| `stride(&self) -> usize` | 一行字节数 = `width * 4` | 手算偏移用 |
| `rgb_offsets(&self) -> (usize, usize, usize)` | R/G/B 在 4 字节内的偏移 | RGBA→`(0,1,2)`,BGRA→`(2,1,0)`。**手撸像素必须用它** |
| `clamp(&self, r: Rect) -> Rect` | 把区域夹进帧边界 | 保证 `x+width<=w`;超出是**静默夹紧**,不报错 |
| `crop(&self, r: Rect) -> Frame` | 裁出子帧(**会拷贝像素**) | 内部先 `clamp`;格式跟随原帧 |
| `to_gray(&self) -> Vec<u8>` | 转灰度(Rec.601 加权),长度 `w*h` | ZNCC 走灰度 |
| `to_gray_into(&self, out: &mut Vec<u8>)` | 同上但复用 `out` 容量 | 循环里用这个,别每帧新建 `Vec` |
| `prepare_bgra(width, height)` / `prepare_rgba(width, height)` | 就地把本帧设成指定尺寸并保证像素长度 | 内部 `clear()` + `resize()`,**容量够就不重新分配**。自己写后端时用它 |

> `Frame` 派生了 `Clone` 和 `Debug`。`Debug` 会把整段像素打出来(吓人且慢),
> 调试时建议只打印 `frame.width`、`frame.height`、`frame.pixels.len()`。

### 8.4 `Template` 与 `Match`

#### 8.4.1 `Template` 的公开字段

```rust
pub struct Template {
    pub rgb: Vec<u8>,            // 每像素 3 字节,固定 RGB
    pub width: usize,
    pub height: usize,
    pub mask: Option<Vec<bool>>, // 掩码:true=参与比较,false=跳过
}
```

`mask` 为 `None` 时全部像素参与比较(向后兼容)。从含 alpha 通道的 PNG 加载时,
`alpha == 0` 的像素会被自动标记为 `false`(不参与比较)。

#### 8.4.2 `Template::load(path: impl AsRef<Path>) -> Result<Self>`

从图片文件读模板,内部转成 RGB(透明通道被丢掉或变成掩码)。

- 参数可以是 `"a.png"`、`String`、`PathBuf`、`Path`——都自动适配。
- **支持 PNG、JPEG、WebP**(v0.8 起)。通过文件头自动识别格式,不需要扩展名正确。
- PNG 若含 alpha 通道且存在 `alpha == 0` 的像素,那些位置会被设为掩码跳过位
  (`RgbMatcher` 比较时忽略),不需要你手动处理。
- 其他格式(BMP/GIF/TIFF)仍返回 `Err(Error::Image(..))`(见 11.4)。
- 透明像素不会变成"忽略该区域"的掩码:非 alpha=0 的透明像素(alpha=1~254)
  仍参与比较,只是 RGB 值按实际存储使用。

```rust
let tpl = Template::load("D:/pic/button.png")?;
let tpl2 = Template::load(std::path::PathBuf::from("button.png"))?;
```

#### 8.4.3 `Template::from_rgb` / `from_rgba` / `with_mask`

```rust
pub fn from_rgb(rgb: Vec<u8>, width: usize, height: usize) -> Self
pub fn from_rgba(rgba: Vec<u8>, width: usize, height: usize) -> Self
pub fn with_mask(self, mask: Vec<bool>) -> Self   // builder-style
```

- `from_rgb`:从内存里的 RGB 字节造模板(无掩码,全部像素参与比较)。
- `from_rgba`:从 RGBA 字节造模板,`alpha == 0` 的像素自动变成掩码跳过位。
- `with_mask`:在已有模板上手动指定掩码(链式调用);`mask.len() == width * height`。

⚠️ `from_rgb` 内部有 `assert_eq!(rgb.len(), width * height * 3)`,长度不对**直接 panic**。
`from_rgba` 同理(`width * height * 4`)。从别处搬来的字节先确认长度。

#### 8.4.4 `Template::content_key(&self) -> u64` 与 `to_gray(&self) -> Vec<u8>`

- `content_key()`:尺寸 + 像素的哈希指纹。库用它决定"能不能复用上次编译好的金字塔 /
  结果缓存是否命中"。你一般用不上,但可以自己拿它判断"模板变没变"(同一个 `Template` 值必然相同)。
- `to_gray()`:Rec.601 转灰度,长度 `width * height`。`CorrMatcher` 内部在用,你自己分析灰度也能调。

#### 8.4.5 `Match`

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match { pub x: i32, pub y: i32, pub score: f32 }
```

- `x` / `y`:**模板左上角**位置。全屏后端 = 屏幕绝对坐标;`WindowCapture` = 窗口相对。
- `score`:`Rgb` 恒为 `1.0`;`Corr` 是 ZNCC 分数(`0.0..=1.0`,越大越像)。
- 是 `Copy` 类型,赋值/传参都是复制,不必纠结借用。
- 中心点:`m.x + tpl.width as i32 / 2`(见 9.1)。

### 8.5 截图后端:`Capture` trait 与四个实现

#### 8.5.1 `Capture` trait 全文

```rust
pub trait Capture {
    fn grab(&mut self) -> Result<Frame>;                                     // 必须实现

    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> { /* 默认:grab 后覆盖 dst,返回 true */ }

    fn grab_region(&mut self, rect: Rect) -> Result<Option<Frame>> { /* 默认:Ok(None) */ }

    fn backend(&self) -> &'static str { "unknown" }                          // 默认名
}
```

| 方法 | 白话 | 你会不会直接调 |
| --- | --- | --- |
| `grab()` | "给我一帧全屏(或整个窗口)" | 会,想自己拿帧的时候 |
| `grab_into(&mut dst)` | "把这一帧塞进我已经准备好的 `dst`,别新建缓冲" | 循环截图时用,省内存分配 |
| `grab_region(rect)` | "**只截这一块**。不支持就返回 `None`" | 一般由 `Finder` 自动调 |
| `backend()` | 后端名字,写日志用 | 排查"我到底在用哪个后端"时很有用 |

`grab_into` 返回的 `bool` 含义是**画面相对上次是否变化**。

- 返回 `false` = "没新画面"。`Finder` 据此跳过搜索(见 8.1.3 的缓存机制)。
- 目前四个后端里**只有 `DxgiCapture` 可能返回 `false`**;`XCapCapture`(用默认实现,恒 `true`)、
  `GdiCapture`、`WindowCapture` 都保守地返回 `true`——注释里写得很清楚:"无从判断是否变化"。

`grab_region` 目前**只有 `XCapCapture` 覆写**了它,而且有条件(见 8.5.2)。

#### 8.5.2 `XCapCapture`(默认后端,跨平台)

```rust
pub struct XCapCapture { /* 内部持有一个 xcap::Monitor */ }

impl XCapCapture {
    pub fn primary() -> Result<Self>;                    // 主显示器;拿不到主屏标记就退化为枚举到的第一个
    pub fn from_point(x: i32, y: i32) -> Result<Self>;   // 包含该屏幕坐标的那台显示器(多屏用)
    pub fn monitor_count() -> Result<usize>;             // 当前在线显示器个数
}
// impl Capture:grab / grab_region / backend -> "xcap"
```

要点:

- 输出的帧是 **RGBA**。
- **一个实例绑一台显示器**,`Monitor` 对象可长期复用(内部只存句柄与几何),`grab` 不会重新枚举显示器。
- `from_point` 是**唯一能拿到副屏**的入口。`CaptureKind` 里没有"选第 N 台显示器"的选项。
- `grab_region(rect)` 的行为:
  - `rect` 是**屏幕绝对坐标**、`usize`。
  - 只有当这台显示器的原点**恰好是 (0,0)**(即主屏)时才真的区域截图;否则返回 `Ok(None)` → 上层回退全屏。
  - 区域为空(`width`/`height` 为 0)或越界(`x+width > 屏宽`)也返回 `Ok(None)`。
  - 成功时返回的帧**原点是 `rect` 左上角**,帧内坐标是相对区域的。`Finder` 会自动把命中坐标加回偏移,
    你自己直接调就要自己加。
- 实测:全屏 ~34ms;`grab_region` 400x300 约 **16.5ms**(区域越小越省)。

```rust
use pixhunt::{Capture, XCapCapture};

let mut cap = XCapCapture::primary()?;
println!("后端:{}", cap.backend());          // xcap
println!("在线显示器:{} 台", XCapCapture::monitor_count()?);

let frame = cap.grab()?;                      // 全屏 RGBA
if let Some(sub) = cap.grab_region((100, 100, 400, 300).into())? {
    println!("区域帧 {}x{}", sub.width, sub.height);   // 400x300
}
```

#### 8.5.3 `GdiCapture` 〔f:capture-gdi〕(仅 Windows)

```rust
impl GdiCapture { pub fn new_primary() -> Self }     // 注意:不返回 Result / Option
// impl Capture:grab / grab_into / backend -> "gdi"
```

- 用 `GetDC(NULL)` + `BitBlt` + `GetDIBits`,输出 **BGRA**。
- 只覆盖主屏;尺寸在构造时由 `GetDeviceCaps(HORZRES/VERTRES)` 决定 —— 分辨率/DPI 缩放改变后
  **需要重建实例**才会跟上。
- ⚠️ **需要知道的行为**:`grab_into` 内部对 `GetDIBits` 失败用的是 `assert!`,也就是说
  真失败时会 **panic**(而不是返回 `Err`)。这是当前实现较粗糙的一处。要稳定请用 `Monitor` 或 `Dxgi`。
- `grab_into` 恒返回 `true`,所以 GDI 后端**享受不到**静态画面跳过。

#### 8.5.4 `DxgiCapture` 〔f:capture-dxgi〕(仅 Windows,最快)

```rust
impl DxgiCapture { pub fn new_primary() -> Option<Self> }   // None = 桌面复制不可用
// impl Capture:grab / grab_into / backend -> "dxgi"
```

- 输出 **BGRA**;`grab_into` 会返回"这帧是不是新画面",静态桌面时能省掉整轮搜索。
- `new_primary()` 返回 `Option`:`None` 意味着**这台机器现在不能用 DXGI**——
  常见原因是远程桌面(RDP)、锁屏、无 GPU、显卡不支持复制。用 `CaptureKind::Dxgi` 时它会变成
  一个明确的 `Err`。
- 另有限制:桌面复制**每秒最多一帧**,轮询频率再高也不会更快(多余轮次得到 `false`)。

```rust
let mut cap = pixhunt::DxgiCapture::new_primary()
    .ok_or_else(|| pixhunt::Error::capture("这台机器 DXGI 不可用"))?;
let mut frame = pixhunt::Frame::bgra8(0, 0, Vec::new());
loop {
    let changed = cap.grab_into(&mut frame)?;
    if !changed { println!("画面没变,跳过"); continue; }
    // …做匹配…
}
```

#### 8.5.5 `WindowCapture` 〔f:capture-window〕(仅 Windows,截单个窗口)

```rust
pub type WindowHandle = isize;         // 就是原生 HWND 的数值形式

impl WindowCapture {
    pub fn new(hwnd: WindowHandle) -> Self;
    pub fn from_title(title: &str) -> Result<Self>;   // 按标题**精确匹配**找顶层窗口
    pub fn size(&self) -> (usize, usize);             // 当前客户区尺寸
}
// impl Capture:grab / grab_into / backend -> "print-window"
```

- 截的是**客户区**(不含标题栏、边框)。
- **被别的窗口挡住也能截**——这是 `PrintWindow` 的本事,全屏后端做不到。
- ⚠️ 命中坐标是**相对该窗口客户区左上角**的,不是屏幕绝对坐标。要点击得先加窗口位置偏移
  (窗口位置要用别的 Win32 代码或别的库拿,pixhunt 不提供)。
- 窗口最小化 / 客户区尺寸为 0 → 返回 `Err(capture error: window has an empty client area (minimized?))`。
- `from_title` 是便捷入口:标题**精确匹配**(不是模糊、不是包含),找不到报
  `no top-level window with title "..."`。生产更推荐你自己拿到句柄后用 `new`。
- 窗口被拖动大小后,`size()` 与帧尺寸会在下一次 `grab` 时自动跟上(`resize_if_needed`)。

```rust
use pixhunt::{Capture, Frame, Matcher, RgbMatcher, Template, WindowCapture};

let mut cap = WindowCapture::from_title("Program Manager")?;   // 桌面窗口
let frame = cap.grab()?;
println!("客户区 {}x{},BGRA {} 字节", frame.width, frame.height, frame.pixels.len());

// 在窗口内找图:坐标是窗口相对
let tpl = Template::load("icon.png")?;
let m = RgbMatcher::new(25).find(&frame, &tpl);
```

它常和 `Finder` 组合成"只盯一个窗口"的找图器:

```rust
let mut finder = Finder::builder()
    .capture(CaptureKind::Window(my_hwnd))
    .matcher(MatchKind::Rgb { tolerance: 25 })
    .build()?;
let m = finder.find_on_screen(&tpl)?;   // 坐标相对该窗口客户区
```

#### 8.5.6 自己写一个后端要做什么

实现 `Capture` 的 `grab` 就够跑通(其余三个都有默认实现)。把实例交给 `Finder::new(Box::new(你的), ..)`。
建议顺手覆写:

- `backend()`:返回你自己的名字,方便日志区分。
- `grab_region`:如果你的底层 API 支持区域抓取,返回 `Some(帧)` 就能让 `Finder` 走快路径
  (注意帧内坐标是**区域相对**的)。
- `grab_into`:**只有当你能判断"画面没变"时才值得覆写**(覆写后返回 `false` 能让 `Finder`
  跳过整轮搜索,这是 `Dxgi` 的真正优势)。单纯为了"复用缓冲"去覆写并不划算——见 10.3,
  省下的分配约 0.006ms,多出来的拷贝约 0.8ms。
- 想让 `Frame` 的存储和你的内部缓冲共享,用 `dst.prepare_bgra(w, h)` / `dst.prepare_rgba(w, h)`:
  容量够时不会重新分配。

### 8.6 匹配器:`Matcher` trait 与 `RgbMatcher`

#### 8.6.1 `Matcher` trait 全文

```rust
pub trait Matcher {
    fn find(&self, frame: &Frame, tpl: &Template) -> Option<Match>;        // 必须实现

    fn find_in(&self, frame: &Frame, tpl: &Template, region: Rect) -> Option<Match> {
        /* 默认:clamp + crop 出子帧,find 后把坐标加回偏移 */
    }

    fn find_all(&self, frame: &Frame, tpl: &Template, region: Rect, max: usize) -> Vec<Match> {
        /* 默认:只转发一个 find 的结果 */
    }
}
```

⚠️ 这两个默认实现的性能含义很重要,是很多"莫名变慢"的根源:

- `find_in` 默认会 **裁剪并拷贝一份子帧**。在没有区域限制时拷一整帧(1080p 约 8MB)纯属浪费。
  `Finder` 内部已经做了规避:能走 `find` 就走 `find`,只有"全屏帧 + 设了 region"才用 `find_in`。
- `find_all` 默认**只返回 1 个结果**。所以凡是没覆写 `find_all` 的匹配器(现在就是 `CorrMatcher`),
  `find_all_on_screen` 只会给你一个命中。

#### 8.6.2 `RgbMatcher`

```rust
pub struct RgbMatcher { pub tolerance: i32 }
impl RgbMatcher { pub fn new(tolerance: i32) -> Self }
// impl Matcher:find / find_in / find_all 全都实现了
```

工作原理(理解了这个就知道它为什么快、什么时候会翻车):

1. 在模板里取**最多 5 个锚点**(中心 + 四角,内缩 2 像素躲开抗锯齿)。
2. 对每个候选位置先比这 5 个点(全在容差内才继续)——绝大多数位置在这一步就被否掉。
3. 锚点过了再**逐像素整窗验证**,一遇超差立刻退出(早失败)。
4. 全程不做灰度转换、不重排通道,通过 `Frame::rgb_offsets()` 直接按帧格式取字节。

所以:

- `score` 恒为 `1.0`(是/否判定,不存在百分比)。
- 结果确定性:总是返回**最上、最左**的那个命中(串行与并行结果完全一致)。
- ⚠️ **退化场景**:如果模板和搜索区域**都几乎是单色**(方差极低),锚点预筛几乎筛不掉任何东西,
  早失败也失效——1080p + 64px 模板实测会从 ~3ms 退化到 **~1.9 秒**。
  避免办法:模板不要从纯色区裁;真要纯色请用颜色搜索(8.8)或 `Corr`(但它也怕零方差)。

`find_all` 的去重规则见 8.1.4。

### 8.7 `CorrMatcher` 与 `CorrConfig` 〔f:match-corr〕

```rust
pub struct CorrMatcher { /* 私有 */ }
impl CorrMatcher {
    pub fn new() -> Self;                        // 默认配置
    pub fn with_config(cfg: CorrConfig) -> Self; // 自定义
}
impl Default for CorrMatcher;                    // 等价于 new()
// impl Matcher:只实现了 find —— find_in / find_all 走默认实现
```

原理:把帧与模板各转一次**灰度**,交给 `corrmatch` 做 **ZNCC(归一化互相关)+ 图像金字塔**
的多层搜索。抗光照变化、抗轻微缩放/形变,但**只做平移匹配**(不认旋转、不做多尺度)。

内部优化(你不用管,但知道无妨):编译好的金字塔模板按 `Template::content_key()` 缓存,
同一模板反复查找不会重编译;灰度缓冲也复用。

```rust
use pixhunt::{CorrConfig, CorrMatcher, MatchKind};

// 简单用
let mut finder = Finder::builder().matcher(MatchKind::Corr).build()?;

// 调参用
let mut finder = Finder::builder()
    .matcher(MatchKind::CorrWith(CorrConfig {
        max_image_levels: 3,     // 目标只在附近小范围移动 → 砍掉深层金字塔
        roi_radius: 4,           // 精修 ROI 缩小
        min_score: 0.7,          // 最终结果门槛
        ..CorrConfig::default()  // 其余用默认
    }))
    .build()?;
```

#### 8.7.1 `CorrConfig` 的五个旋钮

```rust
pub struct CorrConfig {
    pub max_image_levels: usize,  // 默认 6
    pub beam_width: usize,        // 默认 8
    pub roi_radius: usize,        // 默认 8
    pub min_score: f32,           // 默认 NEG_INFINITY(不过滤)
    pub parallel: bool,           // 默认跟随 feature `parallel`
}
impl Default for CorrConfig;
```

| 字段 | 默认 | 调大的效果 | 调小的效果 | 什么时候调 |
| --- | --- | --- | --- | --- |
| `max_image_levels` | `6` | 更慢,但大位移/缩放更稳 | **明显更快**,但目标跑太远会漏 | 已知目标只小幅移动,砍到 3~4 |
| `beam_width` | `8` | 每层保留候选更多,更稳更慢 | 更快,但易丢正确候选 | 误报多→调大;追求延迟→调到 4 |
| `roi_radius` | `8` | 精修范围大,更稳 | 更快,位移大时精修不到位 | 目标几乎不动→调到 4 |
| `min_score` | 不过滤 | 更严(假命中少) | 更松 | 想过滤"勉强像"的结果,常取 0.6~0.8 |
| `parallel` | 跟 feature | — | — | 见下方警告 |

⚠️ 三个必须知道的规则:

1. **所有非法值都会被静默夹到安全下限**(而不是报错):`max_image_levels`/`beam_width`/`roi_radius`
   最小 1;`min_score` 是 `NaN` 或负无穷 → 归为"不过滤",正无穷 → 钳到 `f32::MAX`(任何结果都不达标)。
   好处是"配错值不会静默找不到",坏处是你写错也收不到提醒。
2. **`min_score` 是"最终结果"的阈值,在 pixhunt 侧把关,不会传给 corrmatch。**
   corrmatch 自己也有个同名参数,但那是**逐金字塔层**的候选门槛;粗筛层因为降采样吃掉了对比度,
   分数天然偏低——拿它当最终阈值会把真命中整条链路削空(表现为"永远找不到")。
3. **`parallel: true` 只在 pixhunt 开了 `parallel` feature 时才生效**,否则会被归一为 `false`。
   这不是摆设:强行 true 会让 corrmatch 的 `validate()` 报 `ParallelUnavailable`,
   结果是**静默找不到**。

#### 8.7.2 `CorrMatcher` 的三条限制

- **怕零方差模板**:纯色模板在 ZNCC 下没有意义,匹配不到属预期(和 `Rgb` 的退化不同,这里是"根本不会命中")。
- **`find_all` 只给一个结果**(没覆写默认实现)。
- 延迟约为 `Rgb` 的 4~5 倍(1080p 串行 ~13.7ms / 并行 ~6.6ms)。

### 8.8 颜色搜索:`ColorSpec`、`ColorBlob`、`find_blobs`、`FindColor`

场景:**"只有颜色、没有图"**——血条还剩多少、状态灯是红是绿、高亮选区在哪。不需要模板文件。

```rust
pub struct ColorSpec { pub rgb: [u8; 3], pub tolerance: u8 }
impl ColorSpec { pub fn new(r: u8, g: u8, b: u8, tolerance: u8) -> Self }

pub struct ColorBlob { pub bounds: Rect, pub area: usize }
impl ColorBlob { pub fn center(&self) -> (i32, i32) }

// 自由函数(pixhunt::color::find_blobs)
pub fn find_blobs(frame: &Frame, spec: &ColorSpec, region: Rect, min_area: usize) -> Vec<ColorBlob>;

// 也挂在 Frame 上(需要 use pixhunt::FindColor)
pub trait FindColor { fn find_blobs(&self, spec: &ColorSpec, region: Rect, min_area: usize) -> Vec<ColorBlob>; }
impl FindColor for Frame;
```

- `ColorSpec.rgb` 永远按 **RGB** 填,和帧实际是 RGBA/BGRA 无关(库自己映射);Alpha 被忽略。
- `tolerance` 是 `u8`(每通道最大绝对差),`0` = 必须精确等于该色。
- "连通"的定义是 **8-邻接**(上下左右 + 四个对角都算连着),所以斜着连起来的像素会被并成一个块。
- `area` 是**匹配像素数**,不等于 `bounds.width * bounds.height`(形状不规则时后者更大)。
- `bounds` 是外接矩形,`center()` 是外接矩形的中心(整数像素)。
- `min_area`:小于这个像素数的块被丢弃(滤抗锯齿毛刺、零星噪点)。
- 结果按 `(bounds.y, bounds.x)` 升序。

```rust
use pixhunt::{ColorSpec, FindColor, Frame, Rect};

let red = ColorSpec::new(220, 30, 30, 30);       // 偏红,每通道容差 30

// A) 通过 Finder 一步到位(会自己截图,坐标为屏幕绝对)
let blobs = finder.find_color_on_screen(&red, 50)?;
for b in &blobs {
    println!("红块 bbox=({}, {}, {}x{}) 面积={} 中心={:?}",
        b.bounds.x, b.bounds.y, b.bounds.width, b.bounds.height, b.area, b.center());
}

// B) 已有帧,自己调(注意要 use FindColor 才有方法)
let blobs = frame.find_blobs(&red, frame.full_rect(), 50);
```

判断血条长度这类"读数"需求,常用套路是:限制一个细长 `region` 覆盖血槽,取最左块的
`bounds.width / 血槽总宽`。

### 8.9 错误处理:`Error`、`Result`

```rust
pub enum Error {
    Io(std::io::Error),                                     // 读写模板文件等
    Image(image::ImageError),                               // 解码图片失败
    Capture {
        message: String,                                    // 哪一步失败
        source: Option<Box<dyn std::error::Error + Send + Sync>>,  // 底层原始错误
    },
}
impl Error {
    pub fn capture(message: impl Into<String>) -> Self;
    pub fn capture_from(message: impl Into<String>,
                        source: impl std::error::Error + Send + Sync + 'static) -> Self;
}
pub type Result<T> = std::result::Result<T, Error>;
```

`Display`(也就是 `to_string()` / `{}`)输出示例:

| 情形 | 输出 |
| --- | --- |
| 找不到窗口 | `capture error: no top-level window with title "Foo"` |
| DXGI 不可用 | `capture error: DXGI desktop duplication unavailable` |
| xcap 抓帧失败(带底层原因) | `capture error: xcap Monitor::capture_image(): <xcap 给的原文>` |
| 文件读不到 | `io error: The system cannot find the file specified. (os error 2)` |
| 不是 PNG | `image error: ...` |

`source()`(v0.7.0 起)能沿链取回原始错误:

```rust
use std::error::Error as _;

fn report(err: &pixhunt::Error) {
    eprintln!("{}", err);                       // 已经把 message + 原因拼成一行
    let mut cur = err.source();                 // 再往深挖一层层原因
    while let Some(e) = cur {
        eprintln!("  caused by: {}", e);
        cur = e.source();
    }
}
```

想按类型判别(例如只重试"截图失败",文件错误直接退出):

```rust
match err {
    pixhunt::Error::Io(_) => println!("模板文件的问题,检查路径"),
    pixhunt::Error::Image(_) => println!("图片解码失败,当前只支持 PNG"),
    pixhunt::Error::Capture { message, .. } => println!("截图失败:{}", message),
}
```

> **v0.6 → v0.7 的破坏性变更**:变体从 `Capture(String)` 改成 `Capture { message, source }`。
> 只做 `e.to_string()` 或 `?` 上抛的代码不用改;`match Error::Capture(s)` 要写成
> `match Error::Capture { message, .. }`。

---

## 第 9 章:任务配方(复制即用)

### 9.1 找到按钮并点击它(pixhunt + enigo)

pixhunt **不管鼠标**,点击交给生态库 [`enigo`](https://crates.io/crates/enigo)。这是官方示例
`examples/wait_and_click.rs` 的做法。

`Cargo.toml`:

```toml
[dependencies]
pixhunt = { version = "0.8", features = ["capture-dxgi"] }
enigo = "0.6"
```

```rust
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use pixhunt::{CaptureKind, Finder, MatchKind, Template};
use std::time::Duration;

fn main() -> pixhunt::Result<()> {
    let tpl = Template::load("D:/pic/button.png")?;

    // 眼睛:等它出现(最多 10 秒)
    let mut finder = Finder::builder()
        .capture(CaptureKind::Auto)
        .matcher(MatchKind::Rgb { tolerance: 25 })
        .build()?;
    let Some(m) = finder.find_until(&tpl, Duration::from_secs(10), Duration::from_millis(80))?
    else {
        println!("超时未找到");
        return Ok(());
    };

    // 手:匹配坐标是模板左上角,加半个模板尺寸才是中心
    let cx = m.x + tpl.width as i32 / 2;
    let cy = m.y + tpl.height as i32 / 2;
    println!("命中 @ ({}, {}),点击中心 ({}, {})", m.x, m.y, cx, cy);

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| pixhunt::Error::capture_from("enigo init failed", e))?;
    enigo.move_mouse(cx, cy, Coordinate::Abs)
        .map_err(|e| pixhunt::Error::capture_from("move_mouse failed", e))?;
    enigo.button(Button::Left, Direction::Click)
        .map_err(|e| pixhunt::Error::capture_from("click failed", e))?;
    Ok(())
}
```

⚠️ 两个坑:

1. **忘了算半尺寸**就会点到按钮左上角外面去。
2. **屏幕缩放不是 100%** 时,像素坐标与点击坐标可能对不上(见 11.7)。

### 9.2 一次截图,判断"现在是哪个界面"

```rust
let home = Template::load("home.png")?;
let login = Template::load("login.png")?;
let setting = Template::load("setting.png")?;

let rs = finder.find_many_on_screen(&[&home, &login, &setting])?;
let which = if rs[0].is_some() {
    "主页"
} else if rs[1].is_some() {
    "登录页"
} else if rs[2].is_some() {
    "设置页"
} else {
    "不认识"
};
println!("当前界面:{}", which);
```

比调用三次 `find_on_screen` **少截两次图**(截图是整个流程里最贵的一步)。

### 9.3 等加载遮罩消失

```rust
let mask = Template::load("loading_mask.png")?;
if finder.wait_gone(&mask, Duration::from_secs(30), Duration::from_millis(200))? {
    println!("遮罩已消失,可以继续了");
} else {
    println!("30 秒了还在转,可能卡住了");
}
```

### 9.4 限定区域提速(最有效的招)

```rust
use pixhunt::Rect;

// 建的时候定死
let mut finder = Finder::builder()
    .region(Rect::new(1200, 800, 700, 400))
    .build()?;

// 或运行中改
finder.set_region(None);
```

实测(1920x1200,xcap 后端):全屏端到端 **~47ms** → 限定 400x300 区域 **~20ms**。
`Monitor` 后端还支持**区域直抓**(只截这一块,而不是截全屏再裁),
区域帧与"截全屏再裁剪"**逐字节一致**,结果坐标也一致。

### 9.5 副屏上找图

`CaptureKind` 里没有"选第 N 台显示器"。正确做法:

```rust
use pixhunt::{Finder, Matcher, RgbMatcher, Template, XCapCapture};

// 用第二个屏幕上的一个坐标去"定位"那块屏
let cap = XCapCapture::from_point(2560, 0)?;
let mut finder = Finder::new(Box::new(cap), Box::new(RgbMatcher::new(25)) as Box<dyn Matcher>);
let hit = finder.find_on_screen(&Template::load("btn.png")?)?;   // 坐标是该屏内的像素坐标
```

⚠️ 当前限制:`Rect` 的字段是 `usize`,表示不了副屏常见的**负原点偏移**,所以在副屏上设 `region`
不会走区域直抓(会回退成"截那块屏的全屏再裁剪")。坐标仍然是该屏内像素坐标,
如果要拼成跨屏统一坐标,你需要自己加显示器原点偏移(`xcap` 可以给你,`pixhunt` 没有暴露)。

### 9.6 只盯一个窗口(它被挡住了也没关系)

```rust
use pixhunt::{CaptureKind, Finder, MatchKind, RgbMatcher, Template, WindowCapture};

// 方式 A:按标题(**精确匹配**)找窗口,直接把它当后端交给 Finder::new
let cap = WindowCapture::from_title("记事本")?;
let mut finder = Finder::new(Box::new(cap), Box::new(RgbMatcher::new(25)));

if let Some(m) = finder.find_on_screen(&Template::load("ok.png")?)? {
    // ⚠️ m 是**相对该窗口客户区左上角**的坐标,不是屏幕坐标
    println!("窗口内位置 ({}, {})", m.x, m.y);
}

// 方式 B:你已经从别处拿到了原生句柄(isize),那就能用 builder
let my_hwnd: pixhunt::WindowHandle = 0x0012_3456;   // 示意值
let mut finder2 = Finder::builder()
    .capture(CaptureKind::Window(my_hwnd))
    .matcher(MatchKind::Rgb { tolerance: 25 })
    .build()?;
let _ = finder2.find_on_screen(&Template::load("ok.png")?)?;
```

> pixhunt **没有**"把标题换成句柄"的公开方法(`from_title` 直接给你 `WindowCapture` 实例)。
> 想要句柄数值得自己调 Win32 `FindWindowW`,或用别的窗口库。

### 9.7 血条 / 进度条读数

```rust
use pixhunt::{ColorSpec, Rect};

let bar = ColorSpec::new(220, 40, 40, 40);      // 血量红
// region 覆盖整条血槽,宽 = 满血时的像素宽
let blobs = finder.find_color_on_screen(&bar, 30)?;
if let Some(b) = blobs.first() {
    const FULL: usize = 200;                     // 满血槽宽度(你自己量)
    println!("血量约 {}%", b.bounds.width * 100 / FULL);
}
```

注意:`b.bounds.width` 是**外接矩形宽度**,如果血条被图标遮挡或断成两截,
用 `b.area / 血条高度` 估更稳。

### 9.8 "为什么找不到?"——开 tracing 看内部发生了什么

库内置了 trace 级诊断事件(后端名、帧尺寸、截图/搜索耗时、是否命中、有没有走缓存)。
需要两步:开 feature + 你在自己程序里装一个"订阅器"。

`Cargo.toml`:

```toml
[dependencies]
pixhunt = { version = "0.8", features = ["tracing"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

```rust
fn main() -> pixhunt::Result<()> {
    // 关键:pixhunt 发的是 trace 级(最细),默认会被过滤掉,所以要把等级放到 TRACE
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    // ……之后照常 find_on_screen,日志里会出现 target="pixhunt" 的行
    Ok(())
}
```

### 9.9 不依赖真屏幕的测试(用假帧)

`find_in_frame` 不需要 `mut`、不截图,配合 `Template::from_rgb` 就能写完全确定的测试:

```rust
#[cfg(test)]
mod tests {
    use pixhunt::{Capture, Finder, Frame, Result, RgbMatcher, Template};

    /// 假后端:永远返回同一帧,完全不碰真屏幕。
    struct FakeCap {
        frame: Frame,
    }
    impl Capture for FakeCap {
        fn grab(&mut self) -> Result<Frame> {
            Ok(self.frame.clone())
        }
    }

    #[test]
    fn finds_block_in_synthetic_frame() {
        let (w, h) = (32usize, 32usize);
        let mut px = vec![0u8; w * h * 4];
        let mut tpl = Vec::new();
        for y in 8..8 + 4 {
            for x in 8..8 + 4 {
                let i = (y * w + x) * 4;
                px[i] = 255;
                px[i + 1] = 0;
                px[i + 2] = 255;
                px[i + 3] = 255;
                tpl.extend_from_slice(&[255, 0, 255]);
            }
        }
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(tpl, 4, 4);

        // 路径 1:走完整 Finder(会调用 grab)
        let mut finder = Finder::new(Box::new(FakeCap { frame: frame.clone() }),
                                     Box::new(RgbMatcher::new(0)));
        let m = finder.find_on_screen(&t).unwrap().expect("应命中");
        assert_eq!((m.x, m.y), (8, 8));

        // 路径 2:不截图,直接在帧上找(注意 find_in_frame 不需要 mut)
        let hit = finder.find_in_frame(&frame, &t).expect("也应命中");
        assert_eq!((hit.x, hit.y), (8, 8));
    }
}
```

> 只要实现了 `Capture::grab`,你就能造任意"假画面"来测自己的逻辑,
> 完全不需要真屏幕——本库自己的测试就是这么写的。

---

## 第 10 章:性能与调参

### 10.1 实测数据(1920x1200,`cargo build --release`,多次取中位数)

**截图环节(整屏)**

| 后端 | 中位耗时 | 最快一次 |
| --- | --- | --- |
| `DxgiCapture`("dxgi") | **16.2ms** | 8.7ms |
| `GdiCapture`("gdi") | 32.3ms | — |
| `XCapCapture`("xcap",默认) | 33.8ms | — |
| `XCapCapture::grab_region` 400x300 | **16.5ms** | — |

**纯匹配环节(不含截图)**

| 场景 | 串行 | 开 `parallel` |
| --- | --- | --- |
| `Rgb` 单目标全屏 | ~2.95ms | **~1.68ms** |
| `Rgb` 多目标 `find_all` | ~4.52ms | **~1.20ms** |
| `Corr`(ZNCC)单目标 | ~13.7ms | **~6.6ms** |

**端到端(截图 + 匹配)**

| 组合 | 耗时 |
| --- | --- |
| `Monitor` + 全屏 | ~47ms |
| `Monitor` + 限定 400x300 区域 | **~20ms** |
| `Dxgi` + 全屏 | ~18ms |

### 10.2 结论:钱花在哪了

- **截图占 90% 以上**。想让找图快,先动后端和区域,别优化算法。
- Windows 上把 `CaptureKind::Monitor` 换成 `Dxgi` 或 `Auto`,几乎白捡一半时间。
- **限定区域**是第二根杠杆(47ms → 20ms),因为你只截/只扫一小块。
- 静态桌面轮询:只有 `Dxgi` 会报"画面没变"并跳过搜索,`Monitor`/`Gdi` 每轮都真截图。
- `parallel` 大约给匹配 1.7~2x(算法本身不变,结果完全一致)。ZNCC 提速更明显(约 2x)。

### 10.3 什么会让它突然变慢(重要)

| 现象 | 原因 | 办法 |
| --- | --- | --- |
| `Rgb` 从 ~3ms 掉到 **秒级** | 模板**和**搜索区域都近乎单色:锚点筛不掉、逐像素早失败失效 | 模板别从纯色区裁;换颜色搜索;或给足纹理 |
| 以为"每帧重新分配缓冲"是瓶颈 | 实测 Windows 上大块 `alloc`+`free` 只有 **~0.006ms**,而 9MB 像素的 `memcpy` 要 **~0.8ms** | 别在这上面动手:让 `XCapCapture` 复用缓冲是**负收益**(多的那次拷贝远大于省下的分配)。要快就换 `Dxgi`(截图本身省 15ms+) |
| 区域设了却没变快 | 在副屏上(原点非 (0,0))会回退全屏 | 见 9.5;或直接 `crop` 后调 `find_in_frame` |
| `Corr` 慢得离谱 | 金字塔层数全开 + 大 ROI | 按 8.7.1 调 `max_image_levels` / `roi_radius` |

### 10.4 release 构建很重要

调试构建(`cargo run` 默认)会慢好几倍。性能相关一律加 `--release`:

```powershell
cargo run --release --example find_on_screen -- D:\pic\button.png
```

关于 `Cargo.toml` 里的 `[profile.release]`(本仓库已写 `lto = true`、`codegen-units = 1`):
Rust 的规则是 **profile 只对整个构建的工作区根生效**。也就是说,本库自带的这段配置
只影响"在本仓库里跑示例 / 跑基准";当你的项目依赖 pixhunt 时,**由你的 `Cargo.toml` 说了算**。
想让自己的程序也吃到跨 crate 优化,在你项目根 `Cargo.toml` 末尾加:

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
```

### 10.5 基准测试怎么跑

仓库自带 criterion 基准(`benches/match.rs`):

```powershell
cargo bench                          # 全部
cargo bench --features parallel      # 看并行效果
```

---

## 第 11 章:常见报错与排查

### 11.1 返回 `Ok(None)`(屏幕上明明有,却找不到)

**`None` 不是错误**,它的意思是"按当前条件没匹配上"。按这个顺序排查(命中率从高到低):

| 顺序 | 检查 | 怎么确认 |
| --- | --- | --- |
| 1 | **模板是不是从当前这台机器、当前这个显示比例截的?** | 把 `tpl.width/height` 打印出来,和眼睛看到的实际大小比 |
| 2 | **截图时有没有缩放?** 系统缩放 125%/150%、或图片被编辑器缩过 | 用原始像素截图(1:1),或见 11.7 |
| 3 | **`tolerance` 太小?** | 从默认 25 试着加到 40~60 看是否出现 |
| 4 | **模板是不是几乎纯色?** | 那 `Corr` 一定找不到,`Rgb` 会极慢;改用颜色搜索(8.8) |
| 5 | **界面是不是变了/有动效?** | 按钮有渐变、闪烁、悬浮态变化时,截一张"最稳定状态"的图 |
| 6 | **区域设错了?** | 打印 `finder.region()`;把 region 设成 `None` 再试一次 |
| 7 | **后端截到的是不是你想的那块屏?** | 打印 `frame.width/height` 与 `capture.backend()`;副屏见 9.5 |

终极手段:先 `capture.grab()` 存一帧,把命中区域画出来或直接对这帧调
`find_in_frame` —— 排除"截图内容不对"和"算法找不到"两类问题。

### 11.2 编译错误:`no variant named 'Dxgi'` / `unresolved import 'pixhunt::CorrMatcher'`

99% 是 **feature 没开**。对照第 6.2 节的表:

```powershell
# 临时验证一下是不是 feature 问题
cargo build --all-features
```

能过就说明是 feature。把需要的写进 `Cargo.toml`:

```toml
pixhunt = { version = "0.8", features = ["capture-dxgi", "match-corr"] }
```

另一种典型:`error: expected 2 arguments` / 找不到 `Window` 变体 —— `CaptureKind::Window(..)`
需要 `capture-window`。

⚠️ 还要确认你**在正确的平台上**:`capture-gdi` / `capture-dxgi` / `capture-window` 只在 Windows
编译出代码,在 Linux/macOS 上开了也不会有这些类型。

### 11.3 Linux 链接失败:`rust-lld: error: unable to find library -lgbm`(或 `-lxcb`、`-legl`)

不是库的 bug,是**系统缺开发包**。装齐(见 6.4):

```bash
sudo apt-get install -y --no-install-recommends \
  pkg-config libclang-dev \
  libxcb1-dev libxrandr-dev libdbus-1-dev \
  libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev
```

`-lgbm` 对应 `libgbm-dev`,`-lxcb` 对应 `libxcb1-dev`,依此类推。
`bindgen` 相关的报错则查 `libclang-dev`。

### 11.4 读不了 JPG / BMP / GIF...

v0.8 起 `Template::load` 已支持 **PNG、JPEG、WebP** 三种格式(通过文件头自动识别)。
如果你用的是 BMP/GIF/TIFF 等其他格式,目前仍不被解码,会返回 `Err(Error::Image(..))`。

解决办法:把模板转成上述三种格式之一(PowerToys 或任何截图工具都能存 PNG/JPG)。

```powershell
# 用 .NET 随手转一下(PowerShell)
Add-Type -AssemblyName System.Drawing
[System.Drawing.Image]::FromFile("D:\pic\a.jpg").Save("D:\pic\a.png", [System.Drawing.Imaging.ImageFormat]::Png)
```

### 11.5 `panic: GetDIBits 失败`(用 `capture-gdi` 时)

GDI 后端在 `GetDIBits` 返回 0 时走的是 `assert!`,会直接 panic(见 8.5.3)。常见诱因:
桌面正在切换(锁屏/休眠唤醒瞬间)、GDI 资源被系统限制。

- 稳妥做法:改用 `CaptureKind::Auto` 或 `Monitor`(它们失败会返回 `Err`)。
- 你的程序要防 panic 中断,可以把找图放在 `std::panic::catch_unwind` 里,或干脆换后端。

### 11.6 `capture error: DXGI desktop duplication unavailable`

这台机器现在不能用桌面复制:**远程桌面(RDP)会话、锁屏界面、无 GPU、显卡驱动不支持**。

```rust
// 用 Auto 让它自动降级,而不是硬选 Dxgi
let finder = Finder::builder().capture(CaptureKind::Auto).build()?;
```

注意 `Auto` 需要开了 `capture-gdi` 或 `capture-dxgi` 才存在(6.2)。

### 11.7 找到了但点偏了(高分屏 / 缩放不是 100%)

现象:屏幕缩放 125%/150% 时,`m.x/m.y` 换算出的点击位置和实际按钮对不上。

原因:截图得到的坐标是**像素坐标**,而点击库(如 enigo)可能按**逻辑坐标**工作;
这两者在缩放不是 100% 时不成 1:1 关系,还受"你的进程是否声明为 DPI-aware"影响。
`pixhunt` 只报像素坐标,**不做任何 DPI 换算**。

先确认问题(把两个数字对比):

```rust
let frame = cap.grab()?;
println!("后端报出的帧尺寸:{}x{}", frame.width, frame.height);
```

- 这个数字**等于**你机器的物理分辨率(比如 2560x1440)→ 你拿到的是物理像素。
- 它**等于**缩放后的逻辑分辨率 → 你拿到的是逻辑像素。

然后按你的点击库的需求处理(乘/除缩放系数,或让进程声明 DPI-aware)。
本项目在 100% 缩放的机器上**无法复现**这个差异,因此没有内置换算 —— 如果你正好踩到,
可以按上面的方法实测后再决定加换算还是加文档。

### 11.8 变得非常慢(几秒一次)

先看 10.3 的表。最常见是**模板与背景都近乎纯色**导致 `Rgb` 退化。快速验证:
换一个明显有纹理、有多个颜色的模板跑一次,如果回到几毫秒,就是这个原因。

### 11.9 `error: rustc 1.8x is not supported by this package`

pixhunt 声明的最低版本(MSRV)是 **1.88**;开 `match-corr` 后是 **1.89**(它的依赖要求)。

```powershell
rustup update stable
rustc --version        # 确认 ≥ 1.88
```

### 11.10 在线文档(docs.rs)打不开或显示构建失败

这不是本 crate 的代码问题:它要构建 `xcap`,而 `xcap` 的构建脚本需要系统库(libclang、
PipeWire 头文件等),docs.rs 环境没有。本地看文档没问题:

```powershell
cargo doc --no-deps --open
```

### 11.11 `cargo publish --dry-run` 拒绝执行

提示有未提交文件时,先 `git add` / `git commit`(cargo 要求发布前工作区干净)。

---

## 第 12 章:完整 API 速查表(附录)

### 12.1 crate 根直接可用(`use pixhunt::X;`)

```
Finder, CaptureKind, MatchKind                 —— finder.rs
Finder(方法):builder, new, find_on_screen, find_center_on_screen, find_all_on_screen,
              find_many_on_screen, find_color_on_screen, find_in_frame, find_until,
              wait_gone, diff_since_last, set_region, region
FinderBuilder(方法,经 Finder::builder() 得到):capture, matcher, region, build

Frame, Rect, PixelFormat                        —— frame.rs
Frame:像素字段 pixels/width/height/format;rgba8, bgra8, prepare_bgra, prepare_rgba,
      full_rect, stride, rgb_offsets, clamp, crop, to_gray, to_gray_into
Rect:x/y/width/height(usize);Rect::new;(usize,usize,usize,usize) → Rect
PixelFormat:Rgba8, Bgra8

Template                                        —— template.rs
Template:字段 rgb/width/height/mask;load, from_rgb, from_rgba, with_mask, content_key, to_gray

Match, Matcher, RgbMatcher                      —— matcher.rs
Match:字段 x(i32)/y(i32)/score(f32)
Matcher:find, find_in, find_all
RgbMatcher:字段 tolerance(i32);new

Capture, XCapCapture                            —— capture.rs
Capture:grab, grab_into, grab_region, backend
XCapCapture:primary, from_point, monitor_count

ColorSpec, ColorBlob, FindColor                 —— color.rs
ColorSpec:字段 rgb([u8;3])/tolerance(u8);new
ColorBlob:字段 bounds(Rect)/area(usize);center
FindColor:find_blobs(给 Frame 加方法)

Error, Result                                   —— error.rs
Error:Io(io::Error), Image(image::ImageError), Capture{message, source}
Error:capture, capture_from;From<io::Error>, From<image::ImageError>
Result<T> = std::result::Result<T, Error>
```

### 12.2 需要 feature 才有

```
〔capture-gdi〕   GdiCapture::new_primary() -> GdiCapture;impl Capture(backend "gdi")
〔capture-dxgi〕  DxgiCapture::new_primary() -> Option<DxgiCapture>;impl Capture(backend "dxgi")
〔capture-window〕WindowHandle = isize
                  WindowCapture::new(WindowHandle) -> WindowCapture
                  WindowCapture::from_title(&str) -> Result<WindowCapture>
                  WindowCapture::size() -> (usize, usize)
                  impl Capture(backend "print-window")
                  CaptureKind::Window(WindowHandle)
                  CaptureKind::WindowByTitle(String)
〔gdi 或 dxgi〕   CaptureKind::Auto
〔match-corr〕    CorrMatcher::new() / with_config(CorrConfig) / Default
                  CorrConfig{max_image_levels, beam_width, roi_radius, min_score, parallel}
                  MatchKind::Corr / MatchKind::CorrWith(CorrConfig)
〔tracing〕       库内部发出 target="pixhunt" 的 trace 事件(需要你自己装 subscriber)
〔parallel〕      RgbMatcher 按行并行;给 corrmatch 传导 rayon 能力
```

### 12.3 只存在于子模块

```
pixhunt::finder::FinderBuilder      (类型名;通常无需命名)
pixhunt::color::find_blobs(frame, spec, region, min_area) -> Vec<ColorBlob>
                                  (自由函数版,等价于 FindColor::find_blobs)
```

### 12.4 公开模块(想按路径引用时)

以下模块都是 `pub`,东西都在根上重导出了,平时**不需要**写模块路径:

```
pixhunt::capture    Capture, XCapCapture
pixhunt::color      ColorSpec, ColorBlob, FindColor, find_blobs
pixhunt::error      Error, Result
pixhunt::finder     Finder, FinderBuilder, CaptureKind, MatchKind
pixhunt::frame      Frame, Rect, PixelFormat
pixhunt::matcher    Match, Matcher, RgbMatcher
pixhunt::template   Template
```

带 feature 的模块(`capture_gdi` / `capture_dxgi` / `capture_window` / `matcher_corr`)
只在对应开关打开时存在,类型同样从根导出。

### 12.5 后端 → 输出格式 / 坐标语义 / 是否支持区域直抓 / 是否报告"画面没变"

| 后端 | 格式 | 命中坐标 | `grab_region` | `grab_into` 返回 `false` |
| --- | --- | --- | --- | --- |
| `XCapCapture` | RGBA | 屏幕绝对 | ✅(仅原点 (0,0)) | ❌ 恒 `true` |
| `GdiCapture` | BGRA | 屏幕绝对 | ❌ | ❌ 恒 `true` |
| `DxgiCapture` | BGRA | 屏幕绝对 | ❌ | ✅ 会报没变 |
| `WindowCapture` | BGRA | **窗口客户区相对** | ❌ | ❌ 恒 `true` |

### 12.6 仓库自带的三个示例

| 文件 | 跑法 | 演示 |
| --- | --- | --- |
| `examples/find_on_screen.rs` | `cargo run --release --example find_on_screen -- path\to\template.png` | 最基础的截一屏找一张图 |
| `examples/wait_and_click.rs` | `cargo run --release --features capture-gdi --example wait_and_click -- path\to\button.png` | 找到后用 enigo 点中心(含 `capture_from` 挂错误源) |
| `examples/window_info.rs` | `cargo run --release --features capture-window --example window_info "Program Manager"` | PrintWindow 截窗口 + "自截自找"验证坐标 |

---

## 第 13 章:术语表

| 术语 | 白话解释 |
| --- | --- |
| **模板(Template)** | 你要在屏幕上找的那张小图 |
| **帧(Frame)** | 一次截图得到的原始像素数据(+宽、高、通道顺序) |
| **后端(Capture)** | "怎么把画面拿到手"的实现:xcap / GDI / DXGI / PrintWindow |
| **匹配器(Matcher)** | "怎么在帧里找模板"的实现:Rgb(逐像素)/ Corr(ZNCC) |
| **区域(Region / Rect)** | 限定只在这块矩形里找,主要为了提速与去歧义 |
| **命中(Match)** | 一次找到的结果:`(x, y, score)`,x/y 是模板**左上角** |
| **容差(tolerance)** | 每个颜色通道允许的最大差值(0~255),越大越宽松 |
| **ZNCC** | 归一化互相关。把窗口内像素减去均值再比较"变化形状",因此抗整体明暗变化 |
| **金字塔(pyramid)** | 把图逐级缩小,先在低分辨率粗筛、再回高分辨率精修,省时间 |
| **连通色块(Blob)** | 颜色都匹配、且彼此挨着(8-邻接)的一坨像素 |
| **feature** | Cargo 的编译期可选开关,决定哪些代码/依赖被编进来 |
| **MSRV** | Minimum Supported Rust Version,能编译本库的最低 Rust 版本(本库 1.88;开 `match-corr` 需 1.89) |
| **BGRA / RGBA** | 每像素 4 字节的存放顺序。不同截图方式给不同顺序,库内部自动映射 |
| **stride** | 一行的字节数。本库固定 `width * 4`(无行末填充) |
| **`Box<dyn Trait>`** | "把一个实现了该 trait 的东西放进盒子",让 `Finder` 不关心具体是哪个后端 |
| **`Option<T>` / `Result<T, E>`** | "可能有/没有" / "可能成功/失败"。Rust 用它代替 null 和异常 |
| **`?`** | 出错就提前返回错误,成功就继续。只能用在返回 `Result`/`Option` 的函数里 |
| **`&mut self`** | 该方法会改动调用者所属的对象,所以变量要 `mut` |
| **panic** | 程序遇到无法继续的硬错误直接中止。本库唯一可能 panic 的点见 11.5 |

---

## 版本迁移备忘(升到 0.7 前要知道的)

| 从 | 到 | 破坏性变更 |
| --- | --- | --- |
| v0.5 | v0.6 | 默认后端从 `screenshots` 换成 `xcap`:`CaptureKind::Screenshots` → `Monitor`;`ScreenshotsCapture` → `XCapCapture`;MSRV 1.75 → **1.88**(开 `match-corr` 需 1.89);限定区域改为优先"直接区域抓取" |
| v0.6 | v0.7 | `Error::Capture(String)` → `Error::Capture { message, source }`;新增 `Error::capture` / `Error::capture_from`;只做 `to_string()` 的代码输出不变 |
| v0.7 | v0.8 | `Template` 新增 `pub mask: Option<Vec<bool>>` 字段——用字面量构造 `Template { rgb, width, height }` 的代码需加 `mask: None`;推荐走工厂方法(`load`/`from_rgb`/`from_rgba`)则无需改动。新增 `find_center_on_screen`、`diff_since_last`、`CaptureKind::WindowByTitle`;`Template::load` 现支持 JPEG/WebP |

## 许可

MIT OR Apache-2.0。

> 本说明书内容由 AI 编码助手依据 **pixhunt 0.8.0 的实际源码**逐个 API 清点后撰写,
> 不保证逐行经过人工细读。若你升级了版本,请以 `cargo doc --no-deps --open` 生成的
> 最新文档和源码为准。
