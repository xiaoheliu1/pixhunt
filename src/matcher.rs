//! 匹配"插座":怎么在一帧里找模板。

use crate::frame::{Frame, Rect};
use crate::template::Template;

/// 一次匹配结果:模板左上角落在 `(x, y)`,`score` ∈ [0,1] 越大越像。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    pub x: i32,
    pub y: i32,
    pub score: f32,
}

/// 可插拔的匹配器。实现 [`Matcher::find`] 即可;区域查找 [`find_in`] 与多结果
/// [`find_all`] 有基于裁剪的默认实现,追求性能者可像 [`RgbMatcher`] 那样覆写。
///
/// [`find_in`]: Matcher::find_in
/// [`find_all`]: Matcher::find_all
pub trait Matcher {
    /// 整帧中找第一个匹配(最上、最左)。
    fn find(&self, frame: &Frame, tpl: &Template) -> Option<Match>;

    /// 在指定区域内查找,返回**绝对坐标**。默认实现:裁剪子帧后 `find`,再加偏移。
    fn find_in(&self, frame: &Frame, tpl: &Template, region: Rect) -> Option<Match> {
        let r = frame.clamp(region);
        let sub = frame.crop(r);
        self.find(&sub, tpl).map(|m| Match {
            x: m.x + r.x as i32,
            y: m.y + r.y as i32,
            score: m.score,
        })
    }

    /// 在区域内找全部不重叠匹配(至多 `max` 个,`max=0` 表示不限)。默认实现退化为单个。
    fn find_all(&self, frame: &Frame, tpl: &Template, region: Rect, max: usize) -> Vec<Match> {
        let mut v = match self.find_in(frame, tpl, region) {
            Some(m) => vec![m],
            None => Vec::new(),
        };
        if max != 0 {
            v.truncate(max);
        }
        v
    }
}

/// 极速 RGB 匹配:多点锚点预筛 + 逐像素早失败,不做灰度转换、不重排通道。
///
/// 依据 [`Frame::rgb_offsets`] 自动适配 RGBA / BGRA 帧;模板恒为 RGB。开 feature
/// `parallel` 时,`find`/`find_all` 用 rayon 按行分块并行(结果与串行完全一致)。
#[derive(Clone, Copy, Debug)]
pub struct RgbMatcher {
    /// 每通道允许的最大绝对差(0=精确,~25≈容差 0.1)。
    pub tolerance: i32,
}

impl RgbMatcher {
    pub fn new(tolerance: i32) -> Self {
        RgbMatcher { tolerance }
    }
}

impl Matcher for RgbMatcher {
    fn find(&self, frame: &Frame, tpl: &Template) -> Option<Match> {
        self.find_in(frame, tpl, frame.full_rect())
    }

    fn find_in(&self, frame: &Frame, tpl: &Template, region: Rect) -> Option<Match> {
        let s = Scan::new(frame, tpl, self.tolerance, region)?;
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            let total = s.y_hi - s.y_lo;
            let nthreads = rayon::current_num_threads().max(1);
            // 分块:块数随核数放大,按 y 升序逐块并行;某块命中即返回最上最左,
            // 从而在保持确定性的同时,避免为顶部命中仍扫完整屏。
            let blocks = (nthreads * 8).max(1).min(total.max(1));
            let per = total.div_ceil(blocks);
            let mut start = 0usize;
            while start < total {
                let y0 = s.y_lo + start;
                let y1 = (y0 + per).min(s.y_hi);
                let hit = (y0..y1)
                    .into_par_iter()
                    .filter_map(|yy| s.row_first(yy).map(|x| (x, yy)))
                    .min_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
                if let Some((x, y)) = hit {
                    return Some(Match {
                        x: x as i32,
                        y: y as i32,
                        score: 1.0,
                    });
                }
                start += per;
            }
            None
        }
        #[cfg(not(feature = "parallel"))]
        {
            for y0 in s.y_lo..s.y_hi {
                if let Some(x0) = s.row_first(y0) {
                    return Some(Match {
                        x: x0 as i32,
                        y: y0 as i32,
                        score: 1.0,
                    });
                }
            }
            None
        }
    }

    fn find_all(&self, frame: &Frame, tpl: &Template, region: Rect, max: usize) -> Vec<Match> {
        let max = if max == 0 { usize::MAX } else { max };
        let s = match Scan::new(frame, tpl, self.tolerance, region) {
            Some(s) => s,
            None => return Vec::new(),
        };
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            let mut raw: Vec<(usize, usize)> = (s.y_lo..s.y_hi)
                .into_par_iter()
                .flat_map(|y0| {
                    s.row_all(y0)
                        .into_iter()
                        .map(move |x0| (x0, y0))
                        .collect::<Vec<(usize, usize)>>()
                })
                .collect();
            raw.sort_unstable();
            dedup(s.tw, s.th, max, raw)
        }
        #[cfg(not(feature = "parallel"))]
        {
            let mut raw: Vec<(usize, usize)> = Vec::new();
            for y0 in s.y_lo..s.y_hi {
                for x0 in s.row_all(y0) {
                    raw.push((x0, y0));
                }
            }
            raw.sort_unstable();
            dedup(s.tw, s.th, max, raw)
        }
    }
}

/// 去掉重叠的匹配(窗口在 x 或 y 方向重叠即视为同一目标),`raw` 需已按 (y,x) 排序。
fn dedup(tw: usize, th: usize, max: usize, raw: Vec<(usize, usize)>) -> Vec<Match> {
    let mut kept: Vec<Match> = Vec::new();
    for (x, y) in raw {
        let conflict = kept
            .iter()
            .any(|k| (k.x - x as i32).abs() < tw as i32 && (k.y - y as i32).abs() < th as i32);
        if conflict {
            continue;
        }
        kept.push(Match {
            x: x as i32,
            y: y as i32,
            score: 1.0,
        });
        if kept.len() >= max {
            break;
        }
    }
    kept
}

/// 一个锚点采样:相对模板左上角的偏移 (sx, sy) 与该点的期望 RGB。
#[derive(Clone, Copy)]
struct Sample {
    sx: usize,
    sy: usize,
    r: i32,
    g: i32,
    b: i32,
}

/// 一次扫描的预计算上下文(帧/模板/区域/锚点)。坐标均为绝对像素。
struct Scan<'a> {
    px: &'a [u8],
    tpl: &'a [u8],
    sw4: usize,
    off: (usize, usize, usize),
    tw: usize,
    th: usize,
    tw3: usize,
    thr: i32,
    samples: Vec<Sample>,
    y_lo: usize,
    y_hi: usize,
    x_lo: usize,
    x_hi: usize,
}

impl<'a> Scan<'a> {
    fn new(frame: &'a Frame, tpl: &'a Template, thr: i32, region: Rect) -> Option<Scan<'a>> {
        let (tw, th) = (tpl.width, tpl.height);
        if tw == 0 || th == 0 {
            return None;
        }
        let r = frame.clamp(region);
        if tw > r.width || th > r.height {
            return None;
        }
        let tw3 = tw * 3;
        // 采样点:中心 + 四角(内缩以避开边缘抗锯齿),都是模板真实像素 ->
        // "全部通过"是"整窗匹配"的必要条件,故预筛不会漏掉真匹配。
        let inset_x = 2.min(tw / 2);
        let inset_y = 2.min(th / 2);
        let cx = tw / 2;
        let cy = th / 2;
        let pts = [
            (cx, cy),
            (inset_x, inset_y),
            (tw - 1 - inset_x, inset_y),
            (inset_x, th - 1 - inset_y),
            (tw - 1 - inset_x, th - 1 - inset_y),
        ];
        let mut samples: Vec<Sample> = Vec::with_capacity(pts.len());
        for (sx, sy) in pts {
            let i = sy * tw3 + sx * 3;
            let s = Sample {
                sx,
                sy,
                r: tpl.rgb[i] as i32,
                g: tpl.rgb[i + 1] as i32,
                b: tpl.rgb[i + 2] as i32,
            };
            if !samples.iter().any(|e| e.sx == s.sx && e.sy == s.sy) {
                samples.push(s);
            }
        }
        Some(Scan {
            px: &frame.pixels,
            tpl: &tpl.rgb,
            sw4: frame.width * 4,
            off: frame.rgb_offsets(),
            tw,
            th,
            tw3,
            thr,
            samples,
            x_lo: r.x,
            x_hi: r.x + (r.width - tw) + 1,
            y_lo: r.y,
            y_hi: r.y + (r.height - th) + 1,
        })
    }

    /// 该行内第一个匹配的左上角 x(从左到右扫描,锚点命中后再整窗验证)。
    fn row_first(&self, y0: usize) -> Option<usize> {
        (self.x_lo..self.x_hi).find(|&x0| self.anchor_ok(y0, x0) && self.verify(y0, x0))
    }

    /// 该行内所有匹配的左上角 x。
    fn row_all(&self, y0: usize) -> Vec<usize> {
        let mut v = Vec::new();
        for x0 in self.x_lo..self.x_hi {
            if self.anchor_ok(y0, x0) && self.verify(y0, x0) {
                v.push(x0);
            }
        }
        v
    }

    /// 多锚点预筛:所有采样点的对应屏幕像素都需在容差内才继续。
    #[inline(always)]
    fn anchor_ok(&self, y0: usize, x0: usize) -> bool {
        let (ro, go, bo) = self.off;
        for s in &self.samples {
            let p = (y0 + s.sy) * self.sw4 + s.sx * 4 + x0 * 4;
            if (self.px[p + ro] as i32 - s.r).abs() > self.thr
                || (self.px[p + go] as i32 - s.g).abs() > self.thr
                || (self.px[p + bo] as i32 - s.b).abs() > self.thr
            {
                return false;
            }
        }
        true
    }

    #[inline(always)]
    fn verify(&self, y0: usize, x0: usize) -> bool {
        let (ro, go, bo) = self.off;
        let w4 = self.tw * 4;
        for ty in 0..self.th {
            let sy = (y0 + ty) * self.sw4 + x0 * 4;
            let ti = ty * self.tw3;
            let srow = &self.px[sy..sy + w4];
            let trow = &self.tpl[ti..ti + self.tw3];
            let mut si = 0usize;
            let mut tj = 0usize;
            for _ in 0..self.tw {
                if (srow[si + ro] as i32 - trow[tj] as i32).abs() > self.thr
                    || (srow[si + go] as i32 - trow[tj + 1] as i32).abs() > self.thr
                    || (srow[si + bo] as i32 - trow[tj + 2] as i32).abs() > self.thr
                {
                    return false;
                }
                si += 4;
                tj += 3;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> Vec<u8> {
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
        px
    }

    /// 在帧内 (tx,ty) 处铺一块纯色,返回按 RGB 顺序取出的模板字节。
    fn paste_block(
        px: &mut [u8],
        w: usize,
        _h: usize,
        tx: usize,
        ty: usize,
        s: usize,
        col: [u8; 3],
    ) -> Vec<u8> {
        let mut tpl = Vec::with_capacity(s * s * 3);
        for y in ty..ty + s {
            for x in tx..tx + s {
                let i = (y * w + x) * 4;
                px[i] = col[0];
                px[i + 1] = col[1];
                px[i + 2] = col[2];
                px[i + 3] = 255;
                tpl.extend_from_slice(&[col[0], col[1], col[2]]);
            }
        }
        tpl
    }

    #[test]
    fn rgb_finds_embedded_region() {
        let (w, h) = (64usize, 64usize);
        let mut px = gradient(w, h);
        let tpl = paste_block(&mut px, w, h, 20, 15, 8, [255, 0, 255]);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(tpl, 8, 8);
        let r = RgbMatcher::new(0).find(&frame, &t).expect("应命中");
        assert_eq!((r.x, r.y), (20, 15));
    }

    #[test]
    fn rgb_handles_bgra_frame() {
        let (w, h) = (48usize, 48usize);
        let mut bgra = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                bgra[i] = (x + y) as u8;
                bgra[i + 1] = y as u8;
                bgra[i + 2] = x as u8;
                bgra[i + 3] = 255;
            }
        }
        let frame = Frame::bgra8(w, h, bgra.clone());
        let mut tpl = Vec::new();
        for y in 10..10 + 6 {
            for x in 10..10 + 6 {
                let i = (y * w + x) * 4;
                tpl.extend_from_slice(&[bgra[i + 2], bgra[i + 1], bgra[i]]);
            }
        }
        let t = Template::from_rgb(tpl, 6, 6);
        let r = RgbMatcher::new(0).find(&frame, &t).expect("BGRA 应命中");
        assert_eq!((r.x, r.y), (10, 10));
    }

    #[test]
    fn rgb_returns_none_when_absent() {
        let (w, h) = (32usize, 32usize);
        let frame = Frame::rgba8(w, h, vec![10u8; w * h * 4]);
        let t = Template::from_rgb(vec![200u8; 4 * 4 * 3], 4, 4);
        assert!(RgbMatcher::new(0).find(&frame, &t).is_none());
    }

    #[test]
    fn find_all_returns_two_matches() {
        let (w, h) = (128usize, 64usize);
        let mut px = gradient(w, h);
        let tpl = paste_block(&mut px, w, h, 6, 6, 10, [10, 200, 20]);
        paste_block(&mut px, w, h, 90, 40, 10, [10, 200, 20]);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(tpl, 10, 10);
        let all = RgbMatcher::new(0).find_all(&frame, &t, frame.full_rect(), 0);
        assert_eq!(all.len(), 2, "应找到两处(重叠已去重)");
        assert_eq!((all[0].x, all[0].y), (6, 6));
        assert_eq!((all[1].x, all[1].y), (90, 40));
    }

    #[test]
    fn find_all_respects_max() {
        let (w, h) = (128usize, 64usize);
        let mut px = gradient(w, h);
        let tpl = paste_block(&mut px, w, h, 6, 6, 10, [10, 200, 20]);
        paste_block(&mut px, w, h, 90, 40, 10, [10, 200, 20]);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(tpl, 10, 10);
        let one = RgbMatcher::new(0).find_all(&frame, &t, frame.full_rect(), 1);
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn region_limits_search_and_returns_absolute_coords() {
        let (w, h) = (80usize, 60usize);
        let mut px = gradient(w, h);
        let tpl = paste_block(&mut px, w, h, 50, 40, 8, [200, 100, 10]);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(tpl, 8, 8);
        let m = RgbMatcher::new(0);
        let miss = m.find_in(&frame, &t, Rect::new(0, 0, 40, 60));
        assert!(miss.is_none());
        let hit = m
            .find_in(&frame, &t, Rect::new(40, 30, 40, 30))
            .expect("区域内应命中");
        assert_eq!((hit.x, hit.y), (50, 40));
    }

    #[test]
    fn find_many_on_one_frame() {
        let (w, h) = (96usize, 48usize);
        let mut px = gradient(w, h);
        let tpl_a = paste_block(&mut px, w, h, 10, 10, 6, [250, 10, 10]);
        let tpl_b = paste_block(&mut px, w, h, 60, 30, 6, [10, 10, 250]);
        let frame = Frame::rgba8(w, h, px);
        let ta = Template::from_rgb(tpl_a, 6, 6);
        let tb = Template::from_rgb(tpl_b, 6, 6);
        let m = RgbMatcher::new(0);
        let ra = m.find(&frame, &ta).expect("a");
        let rb = m.find(&frame, &tb).expect("b");
        assert_eq!((ra.x, ra.y), (10, 10));
        assert_eq!((rb.x, rb.y), (60, 30));
    }

    /// 帧内贴一块纯色,但模板每个通道比帧亮 `delta`:tolerance >= delta 应命中,
    /// tolerance < delta 应找不到(锚点预筛与整窗验证都走同一容差)。
    fn tolerance_case(delta: i32) -> (Frame, Template) {
        let (w, h) = (48usize, 48usize);
        let mut px = vec![0u8; w * h * 4];
        for y in 9..9 + 8 {
            for x in 12..12 + 8 {
                let i = (y * w + x) * 4;
                px[i] = 200;
                px[i + 1] = 100;
                px[i + 2] = 50;
                px[i + 3] = 255;
            }
        }
        let bright = |v: i32| (v + delta).clamp(0, 255) as u8;
        let mut tpl = Vec::with_capacity(8 * 8 * 3);
        for _ in 0..8 * 8 {
            tpl.extend_from_slice(&[bright(200), bright(100), bright(50)]);
        }
        (Frame::rgba8(w, h, px), Template::from_rgb(tpl, 8, 8))
    }

    #[test]
    fn tolerance_accepts_small_channel_diff() {
        let (frame, t) = tolerance_case(4);
        let m = RgbMatcher::new(5)
            .find(&frame, &t)
            .expect("容差 5 应吸收 +4 偏差");
        assert_eq!((m.x, m.y), (12, 9));
        // 容差为 0 时同一组像素必须判不匹配,证明命中不是"精确相等"混出来的。
        assert!(
            RgbMatcher::new(0).find(&frame, &t).is_none(),
            "tolerance=0 不应命中带偏差的模板"
        );
    }

    #[test]
    fn tolerance_rejects_large_channel_diff() {
        let (frame, t) = tolerance_case(20);
        assert!(
            RgbMatcher::new(5).find(&frame, &t).is_none(),
            "偏差 20 超出容差 5,不应命中"
        );
    }
}
