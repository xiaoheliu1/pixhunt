//! 颜色范围搜索:不依赖模板图,找画面里所有"接近某颜色"的连通色块。
//!
//! 典型场景:血条/进度条、状态灯(红=离线 绿=在线)、UI 高亮区、工业画面定位。
//! 实现:逐行生成匹配 run(游程),与上一行 run 做 8-邻接合并(并查集),
//! 最终按连通域聚合出包围盒与面积。通道序经 [`Frame::rgb_offsets`] 自适应
//! RGBA / BGRA 帧;区域外像素与 alpha 通道一律忽略。
//!
//! ```
//! use pixhunt::{color::find_blobs, ColorSpec, Frame, Rect};
//!
//! // 16x16 黑底,在 (2,3) 画一块 4x4 纯红。
//! let mut px = vec![0u8; 16 * 16 * 4];
//! for y in 3..7 {
//!     for x in 2..6 {
//!         let i = (y * 16 + x) * 4;
//!         px[i] = 255;
//!         px[i + 3] = 255;
//!     }
//! }
//! let frame = Frame::rgba8(16, 16, px);
//! let blobs = find_blobs(
//!     &frame,
//!     &ColorSpec::new(255, 0, 0, 8),
//!     Rect::new(0, 0, 16, 16),
//!     4,
//! );
//! assert_eq!(blobs.len(), 1);
//! assert_eq!(blobs[0].bounds, Rect::new(2, 3, 4, 4));
//! assert_eq!(blobs[0].area, 16);
//! assert_eq!(blobs[0].center(), (4, 5));
//! ```

use crate::frame::{Frame, Rect};

/// 搜索目标:一个 RGB 颜色加每通道允许的最大绝对差。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorSpec {
    /// 期望颜色(RGB 顺序,与帧的 BGRA/RGBA 无关)。
    pub rgb: [u8; 3],
    /// 每通道容差(0 = 精确等于该色)。
    pub tolerance: u8,
}

impl ColorSpec {
    pub fn new(r: u8, g: u8, b: u8, tolerance: u8) -> Self {
        ColorSpec {
            rgb: [r, g, b],
            tolerance,
        }
    }
}

/// 一个连通色块的几何信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorBlob {
    /// 色块外接矩形(绝对像素坐标)。
    pub bounds: Rect,
    /// 匹配像素数(≠ 面积上限为 bounds 宽高之积)。
    pub area: usize,
}

impl ColorBlob {
    /// 包围盒中心(整数像素)。
    pub fn center(&self) -> (i32, i32) {
        (
            self.bounds.x as i32 + self.bounds.width as i32 / 2,
            self.bounds.y as i32 + self.bounds.height as i32 / 2,
        )
    }
}

/// 每行的一段匹配游程:`label` 是它在并查集里的编号。
struct Run {
    x0: usize,
    x1: usize, // 含
    label: usize,
}

struct Node {
    parent: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    area: usize,
}

fn find(nodes: &mut [Node], mut i: usize) -> usize {
    while nodes[i].parent != i {
        let p = nodes[i].parent;
        nodes[i].parent = nodes[p].parent; // 路径减半
        i = nodes[i].parent;
    }
    i
}

fn union(nodes: &mut [Node], a: usize, b: usize) {
    let (ra, rb) = (find(nodes, a), find(nodes, b));
    if ra == rb {
        return;
    }
    let (keep, drop) = if ra < rb { (ra, rb) } else { (rb, ra) };
    nodes[drop].parent = keep;
    // bbox/area 只在收集阶段合并,这里仅记 parent。
}

/// 在 `frame` 的 `region` 内找所有匹配 `spec` 的 8-连通色块,
/// 丢弃面积 < `min_area` 的碎点;结果按 (顶边 y, 左边 x) 升序。
pub fn find_blobs(
    frame: &Frame,
    spec: &ColorSpec,
    region: Rect,
    min_area: usize,
) -> Vec<ColorBlob> {
    let r = frame.clamp(region);
    let (ro, go, bo) = frame.rgb_offsets();
    let tol = spec.tolerance as i32;
    let (tr, tg, tb) = (spec.rgb[0] as i32, spec.rgb[1] as i32, spec.rgb[2] as i32);
    let sw4 = frame.width * 4;
    let x_end = r.x + r.width;

    let mut nodes: Vec<Node> = Vec::new();
    let mut prev: Vec<Run> = Vec::new();
    let mut cur: Vec<Run> = Vec::new();

    for y in r.y..r.y + r.height {
        cur.clear();
        let base = y * sw4;
        let mut x = r.x;
        while x < x_end {
            let i = base + x * 4;
            let px = &frame.pixels[i..i + 4];
            if (px[ro] as i32 - tr).abs() <= tol
                && (px[go] as i32 - tg).abs() <= tol
                && (px[bo] as i32 - tb).abs() <= tol
            {
                let s = x;
                x += 1;
                while x < x_end {
                    let i = base + x * 4;
                    let px = &frame.pixels[i..i + 4];
                    if (px[ro] as i32 - tr).abs() > tol
                        || (px[go] as i32 - tg).abs() > tol
                        || (px[bo] as i32 - tb).abs() > tol
                    {
                        break;
                    }
                    x += 1;
                }
                let label = nodes.len();
                nodes.push(Node {
                    parent: label,
                    x0: s,
                    y0: y,
                    x1: x - 1,
                    y1: y,
                    area: x - s,
                });
                cur.push(Run {
                    x0: s,
                    x1: x - 1,
                    label,
                });
            } else {
                x += 1;
            }
        }
        // 8-邻接:与上一行 x 区间(左右各扩 1)相交的 run 合并。
        //
        // `prev` 与 `cur` 都是按 x 升序生成的(run 互不相交),所以双指针一遍扫完:
        // 完全落在当前 run 左边的 prev run 以后只会更左,直接丢掉。旧的写法对每对
        // (c, p) 都判一次,细密条纹/棋盘格画面上是 O(H·R²):1920x1200、2px 格子
        // (每行 480 个 run)、`--release` 实测 157 ms,改完同一画面 39 ms ——
        // 剩下的基本就是逐像素扫描本身加 27 万个 run 的分配。
        let mut first = 0usize;
        for c in &cur {
            while first < prev.len() && prev[first].x1 + 1 < c.x0 {
                first += 1;
            }
            let mut k = first;
            while k < prev.len() && prev[k].x0 <= c.x1 + 1 {
                union(&mut nodes, c.label, prev[k].label);
                k += 1;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    // 按根节点聚合 bbox 与面积。
    let mut roots: Vec<Node> = Vec::new(); // 复用 Node 作累加器
    let mut index: Vec<i32> = vec![-1; nodes.len()];
    for i in 0..nodes.len() {
        let root = find(&mut nodes, i);
        let n = &nodes[i];
        if index[root] < 0 {
            index[root] = roots.len() as i32;
            roots.push(Node {
                parent: root,
                x0: n.x0,
                y0: n.y0,
                x1: n.x1,
                y1: n.y1,
                area: 0,
            });
        }
        let slot = index[root] as usize;
        let a = &mut roots[slot];
        a.x0 = a.x0.min(n.x0);
        a.y0 = a.y0.min(n.y0);
        a.x1 = a.x1.max(n.x1);
        a.y1 = a.y1.max(n.y1);
        a.area += n.area;
    }

    let mut blobs: Vec<ColorBlob> = roots
        .into_iter()
        .filter(|n| n.area >= min_area.max(1))
        .map(|n| ColorBlob {
            bounds: Rect::new(n.x0, n.y0, n.x1 - n.x0 + 1, n.y1 - n.y0 + 1),
            area: n.area,
        })
        .collect();
    blobs.sort_unstable_by_key(|b| (b.bounds.y, b.bounds.x));
    blobs
}

/// [`Frame::find_blobs`] 的自由函数等价方法(便于 `use pixhunt::FindColor` 链式调用)。
pub trait FindColor {
    fn find_blobs(&self, spec: &ColorSpec, region: Rect, min_area: usize) -> Vec<ColorBlob>;
}

impl FindColor for Frame {
    fn find_blobs(&self, spec: &ColorSpec, region: Rect, min_area: usize) -> Vec<ColorBlob> {
        find_blobs(self, spec, region, min_area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 64;
    const H: usize = 48;

    /// 黑底 RGBA 帧 + 给定 RGB 块列表 (x, y, w, h, color)。
    fn scene(blocks: &[(usize, usize, usize, usize, [u8; 3])]) -> Frame {
        let mut px = vec![0u8; W * H * 4];
        for p in px.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        for &(x, y, w, h, col) in blocks {
            for yy in y..y + h {
                for xx in x..x + w {
                    let i = (yy * W + xx) * 4;
                    px[i] = col[0];
                    px[i + 1] = col[1];
                    px[i + 2] = col[2];
                }
            }
        }
        Frame::rgba8(W, H, px)
    }

    const RED: [u8; 3] = [255, 0, 0];

    #[test]
    fn finds_two_separate_blobs() {
        let f = scene(&[(10, 10, 20, 8, RED), (40, 30, 8, 8, RED)]);
        let blobs = find_blobs(&f, &ColorSpec::new(255, 0, 0, 10), (0, 0, W, H).into(), 1);
        assert_eq!(blobs.len(), 2);
        assert_eq!(blobs[0].bounds, Rect::new(10, 10, 20, 8));
        assert_eq!(blobs[0].area, 160);
        assert_eq!(blobs[1].bounds, Rect::new(40, 30, 8, 8));
        assert_eq!(blobs[1].center(), (44, 34));
    }

    #[test]
    fn filters_by_min_area() {
        let f = scene(&[(10, 10, 20, 8, RED), (40, 30, 4, 4, RED)]);
        let blobs = find_blobs(&f, &ColorSpec::new(255, 0, 0, 10), (0, 0, W, H).into(), 100);
        assert_eq!(blobs.len(), 1, "16 像素的碎点应被 min_area=100 过滤");
        assert_eq!(blobs[0].area, 160);
    }

    #[test]
    fn diagonal_pixels_are_one_blob() {
        // 8-邻接:两个对角相触的格子属于同一连通域。
        let f = scene(&[(20, 20, 2, 2, RED), (22, 22, 2, 2, RED)]);
        let blobs = find_blobs(&f, &ColorSpec::new(255, 0, 0, 0), (0, 0, W, H).into(), 1);
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].bounds, Rect::new(20, 20, 4, 4));
        assert_eq!(blobs[0].area, 8);
    }

    #[test]
    fn tolerance_decides_near_miss() {
        // 目标色比帧色每通道亮 6:tol=10 命中,tol=3 落空。
        let f = scene(&[(5, 5, 10, 10, [100, 100, 100])]);
        let spec_loose = ColorSpec::new(106, 106, 106, 10);
        let spec_strict = ColorSpec::new(106, 106, 106, 3);
        assert_eq!(find_blobs(&f, &spec_loose, (0, 0, W, H).into(), 1).len(), 1);
        assert!(find_blobs(&f, &spec_strict, (0, 0, W, H).into(), 1).is_empty());
    }

    #[test]
    fn works_on_bgra_frames() {
        // 同内容构造 BGRA 帧,结果应与 RGBA 帧一致(通道自适应)。
        let rgba = scene(&[(10, 10, 20, 8, RED)]);
        let mut bgra_px = rgba.pixels.clone();
        for p in bgra_px.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        let bgra = Frame::bgra8(W, H, bgra_px);
        let spec = ColorSpec::new(255, 0, 0, 5);
        let a = find_blobs(&rgba, &spec, (0, 0, W, H).into(), 1);
        let b = find_blobs(&bgra, &spec, (0, 0, W, H).into(), 1);
        assert_eq!(a, b);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn region_limits_detection() {
        let f = scene(&[(10, 10, 20, 8, RED)]);
        let spec = ColorSpec::new(255, 0, 0, 5);
        // 区域完全错过色块。
        assert!(find_blobs(&f, &spec, (35, 5, 20, 20).into(), 1).is_empty());
        // 子区域命中,坐标仍是绝对。
        let b = find_blobs(&f, &spec, (0, 0, 32, 24).into(), 1);
        assert_eq!(b[0].bounds, Rect::new(10, 10, 20, 8));
    }

    /// 逐像素 flood fill 参照 vs 运行(run)合并:随机噪声图上两者的连通块必须
    /// **逐个相同**。8-邻接的判定条件只差一个边界(`p.x0 <= c.x1+1 && p.x1+1 >= c.x0`)
    /// 就会漏合并或错合并,bbox 与面积立即对不上,所以这个参照同时能顶住双指针
    /// 里 `first`/`k` 的两个移动步骤。
    #[test]
    fn blobs_match_pixel_flood_fill_reference() {
        let (w, h) = (48usize, 36usize);
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        // 从"整幅全匹配"(den=1,一个巨块)到"很稀疏"(den=40,大量碎点)都盖到。
        for den in [1usize, 3, 5, 9, 40] {
            // 不用 `move`:闭包必须借用外层的 `seed`,噪声才会在各档之间接着往下推,
            // 而不是每档都从同一个种子重算一遍同样的图。
            let mut rnd = || {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (seed >> 33) as usize
            };
            let mut hit = vec![false; w * h];
            let mut px = vec![0u8; w * h * 4];
            for (i, hv) in hit.iter_mut().enumerate() {
                let on = rnd() % den == 0;
                *hv = on;
                let v = if on { 255u8 } else { 0u8 };
                px[i * 4] = v;
                px[i * 4 + 1] = v;
                px[i * 4 + 2] = v;
                px[i * 4 + 3] = 255;
            }

            // 参照:对每个未访问的匹配像素做 8-连通 BFS,统计 bbox 与像素数。
            let mut seen = vec![false; w * h];
            let mut want: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
            for start in 0..w * h {
                if !hit[start] || seen[start] {
                    continue;
                }
                seen[start] = true;
                let mut queue = vec![start];
                let (mut x0, mut y0) = (w, h);
                let (mut x1, mut y1) = (0usize, 0usize);
                let mut area = 0usize;
                while let Some(idx) = queue.pop() {
                    let (cx, cy) = (idx % w, idx / w);
                    x0 = x0.min(cx);
                    y0 = y0.min(cy);
                    x1 = x1.max(cx);
                    y1 = y1.max(cy);
                    area += 1;
                    for dy in [-1i32, 0, 1] {
                        for dx in [-1i32, 0, 1] {
                            let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                            if nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                                continue;
                            }
                            let ni = ny as usize * w + nx as usize;
                            if hit[ni] && !seen[ni] {
                                seen[ni] = true;
                                queue.push(ni);
                            }
                        }
                    }
                }
                want.push((x0, y0, x1 - x0 + 1, y1 - y0 + 1, area));
            }
            want.sort_by_key(|blob| (blob.1, blob.0));

            let f = Frame::rgba8(w, h, px);
            let spec = ColorSpec::new(255, 255, 255, 0);
            let got = find_blobs(&f, &spec, f.full_rect(), 1);
            assert_eq!(got.len(), want.len(), "den={den} 连通块个数不一致");
            for (g, e) in got.iter().zip(want.iter()) {
                assert_eq!(
                    (
                        g.bounds.x,
                        g.bounds.y,
                        g.bounds.width,
                        g.bounds.height,
                        g.area
                    ),
                    *e,
                    "den={den} 某个连通块的 bbox/面积不一致"
                );
            }
        }
    }
}
