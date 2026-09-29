//! 找图热路径基准:`RgbMatcher` 在 ~1080p 全屏上的单帧查找与多结果查找;
//! `CorrMatcher`(feature `match-corr`)另测冷启动(含模板编译)。
//!
//! 运行:
//!   cargo bench --features parallel         # 并行路径
//!   cargo bench                             # 串行路径
//!   cargo bench --features match-corr,parallel   # ZNCC 路径

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pixhunt::{Frame, Matcher, RgbMatcher, Template};

fn make_frame(w: usize, h: usize) -> Frame {
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            px[i] = (x % 251) as u8;
            px[i + 1] = (y % 253) as u8;
            px[i + 2] = ((x + y) % 249) as u8;
            px[i + 3] = 255;
        }
    }
    Frame::rgba8(w, h, px)
}

/// 在 (tx,ty) 贴一块纯色,返回其 RGB 模板字节。
fn embed(frame: &mut Frame, tx: usize, ty: usize, s: usize) -> Vec<u8> {
    let w = frame.width;
    let mut rgb = Vec::with_capacity(s * s * 3);
    for y in ty..ty + s {
        for x in tx..tx + s {
            let i = (y * w + x) * 4;
            frame.pixels[i] = 250;
            frame.pixels[i + 1] = 5;
            frame.pixels[i + 2] = 200;
            rgb.extend_from_slice(&[250, 5, 200]);
        }
    }
    rgb
}

fn bench_find(c: &mut Criterion) {
    let (w, h) = (1920usize, 1080usize);
    let mut frame = make_frame(w, h);
    let rgb = embed(&mut frame, 1200, 640, 64);
    let tpl = Template::from_rgb(rgb, 64, 64);
    let m = RgbMatcher::new(0);
    c.bench_function("rgb_find_fullscreen_1080p", |b| {
        b.iter(|| black_box(&m).find(black_box(&frame), black_box(&tpl)))
    });
}

fn bench_find_all(c: &mut Criterion) {
    let (w, h) = (1920usize, 1080usize);
    let mut frame = make_frame(w, h);
    let rgb = embed(&mut frame, 300, 200, 48);
    embed(&mut frame, 1500, 800, 48);
    let tpl = Template::from_rgb(rgb, 48, 48);
    let m = RgbMatcher::new(0);
    let region = frame.full_rect();
    c.bench_function("rgb_find_all_fullscreen_1080p", |b| {
        b.iter(|| m.find_all(black_box(&frame), black_box(&tpl), black_box(region), 0))
    });
}

/// 有纹理的 1080p 帧 + 贴在 (tx,ty) 的 `s`px 模板(供 ZNCC 使用,需局部对比度)。
#[cfg(feature = "match-corr")]
fn textured_scene(s: usize) -> (Frame, Template) {
    let (w, h) = (1920usize, 1080usize);
    let (tx, ty) = (1200usize, 640usize);
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            px[i] = ((x * 7 + y * 13) % 256) as u8;
            px[i + 1] = ((x * 3 + y * 11) % 256) as u8;
            px[i + 2] = ((x * 5 + y * 17) % 256) as u8;
            px[i + 3] = 255;
        }
    }
    let mut rgb = Vec::with_capacity(s * s * 3);
    for yy in 0..s {
        for xx in 0..s {
            let i = ((ty + yy) * w + (tx + xx)) * 4;
            let checker = if (xx / 4 + yy / 4) % 2 == 0 { 240 } else { 15 };
            let r = (checker - xx as i32 * 2).clamp(0, 255) as u8;
            let g = (checker - yy as i32 * 2).clamp(0, 255) as u8;
            let b = ((xx * 5 + yy * 3) % 256) as u8;
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            rgb.extend_from_slice(&[r, g, b]);
        }
    }
    (Frame::rgba8(w, h, px), Template::from_rgb(rgb, s, s))
}

/// ZNCC 热路径(模板已缓存)与冷启动(每次新建匹配器,含模板编译)。
#[cfg(feature = "match-corr")]
fn bench_corr(c: &mut Criterion) {
    use pixhunt::CorrMatcher;

    let (frame, tpl) = textured_scene(64);
    let m = CorrMatcher::new();
    m.find(&frame, &tpl).expect("warmup");
    c.bench_function("corr_find_fullscreen_1080p", |b| {
        b.iter(|| black_box(&m).find(black_box(&frame), black_box(&tpl)))
    });

    c.bench_function("corr_cold_find_fullscreen_1080p", |b| {
        b.iter(|| {
            let m = CorrMatcher::new();
            m.find(black_box(&frame), black_box(&tpl)).is_some()
        })
    });
}

#[cfg(feature = "match-corr")]
criterion_group!(benches, bench_find, bench_find_all, bench_corr);
#[cfg(not(feature = "match-corr"))]
criterion_group!(benches, bench_find, bench_find_all);

criterion_main!(benches);
