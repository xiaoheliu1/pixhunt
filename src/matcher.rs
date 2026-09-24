//! 匹配"插座":怎么在一帧里找模板。

use crate::frame::Frame;
use crate::template::Template;

/// 一次匹配结果:模板左上角落在 `(x, y)`,`score` ∈ [0,1] 越大越像。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    pub x: i32,
    pub y: i32,
    pub score: f32,
}

/// 可插拔的匹配器。实现它即可换一种找法(如 corrmatch 的 ZNCC)。
pub trait Matcher {
    fn find(&self, frame: &Frame, tpl: &Template) -> Option<Match>;
}

/// 极速 RGB 匹配:锚点预筛 + 逐像素早失败,不做灰度转换、不重排通道。
///
/// 依据 [`Frame::rgb_offsets`] 自动适配 RGBA / BGRA 帧;模板恒为 RGB。
/// 适合"屏幕内容与模板几乎一致"的场景(快);光照/缩放变化大的场景用 ZNCC。
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
        let (ro, go, bo) = frame.rgb_offsets();
        let (x, y) = find_rgb(
            &frame.pixels,
            frame.width,
            frame.height,
            (ro, go, bo),
            &tpl.rgb,
            tpl.width,
            tpl.height,
            self.tolerance,
        )?;
        Some(Match { x, y, score: 1.0 })
    }
}

/// 在 screen(每像素4字节, 通道偏移 ro/go/bo)中找 tpl(RGB, 每像素3字节)。
/// 以模板中心像素为锚点先筛颜色,命中再整窗逐像素验证(任一模失配立即失败)。
fn find_rgb(
    screen: &[u8],
    sw: usize,
    sh: usize,
    (ro, go, bo): (usize, usize, usize),
    tpl: &[u8],
    tw: usize,
    th: usize,
    thr: i32,
) -> Option<(i32, i32)> {
    if tw == 0 || th == 0 || tw > sw || th > sh {
        return None;
    }
    let sw4 = sw * 4;
    let tw3 = tw * 3;
    let ax = tw / 2;
    let ay = th / 2;
    let ai = ay * tw3 + ax * 3;
    let (ar, ag, ab) = (tpl[ai] as i32, tpl[ai + 1] as i32, tpl[ai + 2] as i32);

    let x_end = sw - tw + 1;
    let y_end = sh - th + 1;

    for y0 in 0..y_end {
        let anchor_row = (y0 + ay) * sw4 + ax * 4;
        for x0 in 0..x_end {
            let p = anchor_row + x0 * 4;
            if (screen[p + ro] as i32 - ar).abs() > thr {
                continue;
            }
            if (screen[p + go] as i32 - ag).abs() > thr {
                continue;
            }
            if (screen[p + bo] as i32 - ab).abs() > thr {
                continue;
            }
            if verify(screen, sw4, (ro, go, bo), tpl, tw, th, tw3, thr, x0, y0) {
                return Some((x0 as i32, y0 as i32));
            }
        }
    }
    None
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn verify(
    screen: &[u8],
    sw4: usize,
    (ro, go, bo): (usize, usize, usize),
    tpl: &[u8],
    tw: usize,
    th: usize,
    tw3: usize,
    thr: i32,
    x0: usize,
    y0: usize,
) -> bool {
    let w4 = tw * 4;
    for ty in 0..th {
        let sy = (y0 + ty) * sw4 + x0 * 4;
        let ti = ty * tw3;
        let srow = &screen[sy..sy + w4];
        let trow = &tpl[ti..ti + tw3];
        let mut si = 0usize;
        let mut tj = 0usize;
        for _ in 0..tw {
            if (srow[si + ro] as i32 - trow[tj] as i32).abs() > thr
                || (srow[si + go] as i32 - trow[tj + 1] as i32).abs() > thr
                || (srow[si + bo] as i32 - trow[tj + 2] as i32).abs() > thr
            {
                return false;
            }
            si += 4;
            tj += 3;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张渐变 RGBA 帧,把其中一块原样作为模板找回来(不依赖显示器,CI 友好)。
    #[test]
    fn rgb_finds_embedded_region() {
        let (w, h) = (64usize, 64usize);
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                px[i] = x as u8;
                px[i + 1] = y as u8;
                px[i + 2] = (x + y) as u8;
                px[i + 3] = 255;
            }
        }
        let frame = Frame::rgba8(w, h, px.clone());
        let tpl = crop_rgb(&px, w, 20, 15, 8, 8);
        let t = Template::from_rgb(tpl, 8, 8);
        let r = RgbMatcher::new(0).find(&frame, &t).expect("应能命中");
        assert_eq!((r.x, r.y), (20, 15));
    }

    /// 同一内容,但帧是 BGRA(如 GDI / DXGI 产出),模板仍 RGB —— 应照样命中。
    #[test]
    fn rgb_handles_bgra_frame() {
        let (w, h) = (48usize, 48usize);
        let mut bgra = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                bgra[i] = (x + y) as u8; // B
                bgra[i + 1] = y as u8; // G
                bgra[i + 2] = x as u8; // R
                bgra[i + 3] = 255;
            }
        }
        let frame = Frame::bgra8(w, h, bgra.clone());
        // 模板按 RGB 顺序取(与帧通道无关)
        let mut tpl = Vec::new();
        for y in 10..10 + 6 {
            for x in 10..10 + 6 {
                let i = (y * w + x) * 4;
                tpl.extend_from_slice(&[bgra[i + 2], bgra[i + 1], bgra[i]]); // R,G,B
            }
        }
        let t = Template::from_rgb(tpl, 6, 6);
        let r = RgbMatcher::new(0).find(&frame, &t).expect("BGRA 应能命中");
        assert_eq!((r.x, r.y), (10, 10));
    }

    #[test]
    fn rgb_returns_none_when_absent() {
        let (w, h) = (32usize, 32usize);
        let px = vec![10u8; w * h * 4];
        let frame = Frame::rgba8(w, h, px);
        let tpl = vec![200u8; 4 * 4 * 3];
        let t = Template::from_rgb(tpl, 4, 4);
        assert!(RgbMatcher::new(0).find(&frame, &t).is_none());
    }

    fn crop_rgb(px: &[u8], w: usize, tx: usize, ty: usize, tw: usize, th: usize) -> Vec<u8> {
        let mut tpl = Vec::with_capacity(tw * th * 3);
        for y in ty..ty + th {
            for x in tx..tx + tw {
                let i = (y * w + x) * 4;
                tpl.extend_from_slice(&[px[i], px[i + 1], px[i + 2]]);
            }
        }
        tpl
    }
}
