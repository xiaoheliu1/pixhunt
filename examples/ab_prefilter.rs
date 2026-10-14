//! 端到端 A/B 测量工具:同一张真机截帧 + 同一个大图模板,逐容差取 12 次中位,
//! 用来核对预筛改动前后的实际耗时与命中坐标是否漂移。
//!
//!   cargo run --release --features parallel --example ab_prefilter -- \
//!       <帧 png> <模板 png>
//!
//! 只用公开 API,所以能同时在改动前/改动后的代码上编译运行 —— 把两个二进制
//! 的输出对排就是一组 A/B。不依赖任何 `pub(crate)` 字段,改动不会让它编不过。

use std::time::Instant;

use pixhunt::{Frame, Matcher, RgbMatcher, Template};

fn main() {
    let mut args = std::env::args().skip(1);
    let frame_path = args.next().expect("用法: ab_prefilter <帧 png> <模板 png>");
    let tpl_path = args.next().expect("用法: ab_prefilter <帧 png> <模板 png>");

    // 帧按 BGRA 建,与安卓侧 Kotlin MediaProjection 注入的像素布局同口径
    // (rgb_offsets 会给出 (2,1,0),内层循环走的偏移路径一致)。
    let t = Template::load(&frame_path).expect("帧读不出来");
    let mut px = Vec::with_capacity(t.width * t.height * 4);
    for c in t.rgb.as_chunks::<3>().0 {
        px.extend_from_slice(&[c[2], c[1], c[0], 255]);
    }
    let frame = Frame::bgra8(t.width, t.height, px);
    let tpl = Template::load(&tpl_path).expect("模板读不出来");
    println!(
        "帧 {}x{}  模板 {}x{}",
        frame.width, frame.height, tpl.width, tpl.height
    );

    for tol in [0i32, 10, 25, 40] {
        let m = RgbMatcher::new(tol);
        // 两枪预热:把首次解码/页缓存/线程池的开销打掉
        let warm1 = m.find(&frame, &tpl);
        let warm2 = m.find(&frame, &tpl);
        let _ = (warm1, warm2);
        let mut times = Vec::new();
        let mut coord = String::new();
        for i in 0..12 {
            let t0 = Instant::now();
            let hit = m.find(&frame, &tpl);
            let dt = t0.elapsed().as_micros() as u64;
            times.push(dt);
            let s = match hit {
                Some(h) => format!("{},{}", h.x, h.y),
                None => "MISS".to_string(),
            };
            if i == 0 {
                coord = s.clone();
            }
            if s != coord {
                println!("  ⚠ tol={tol} 第 {i} 次命中坐标漂了: {s} (首次 {coord})");
            }
        }
        times.sort_unstable();
        let sum: u64 = times.iter().sum();
        println!(
            "tol={tol:<3} 中位={}us  最快={}us  平均={}us  最慢={}us  命中={}",
            times[times.len() / 2],
            times[0],
            sum / times.len() as u64,
            times[times.len() - 1],
            coord
        );
    }
}
