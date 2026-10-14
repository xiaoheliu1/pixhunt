//! 匹配"插座":怎么在一帧里找模板。

use std::collections::VecDeque;

use crate::frame::{Frame, Rect};
use crate::template::{Anchor, Template};

/// 一次匹配结果:模板左上角落在 `(x, y)`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    pub x: i32,
    pub y: i32,
    /// 相似度。**口径随匹配器不同,别拿它跨匹配器比较**:
    ///
    /// - [`RgbMatcher`]:恒为 `1.0`。它是"容差内逐像素全对才算命中"的通过/不通过
    ///   判定,不存在"90% 像",所以这个字段不带信息,不要用它筛"最像的那个"。
    /// - `CorrMatcher`(ZNCC,需 feature `match-corr`):`0.0..=1.0`,越大越像
    ///   (1.0 完美,0.7 已算很强,0.3 基本是噪声)。要分数就用 `MatchKind::Corr`。
    pub score: f32,
}

impl Match {
    /// 以 `tpl` 的尺寸折算出**模板中心**坐标(整数除法,与
    /// [`Finder::find_center_on_screen`](crate::Finder::find_center_on_screen) 同口径)。
    ///
    /// 单结果直接用
    /// [`Finder::find_center_on_screen`](crate::Finder::find_center_on_screen);
    /// 这个入口是为 [`Matcher::find_all`] 拿到一批左上角、又想逐个中心点击准备的。
    ///
    /// ```
    /// use pixhunt::{Match, Template};
    ///
    /// let tpl = Template::from_rgb(vec![0u8; 10 * 10 * 3], 10, 10);
    /// let m = Match {
    ///     x: 20,
    ///     y: 30,
    ///     score: 1.0,
    /// };
    /// assert_eq!(m.center(&tpl), (25, 35));
    /// ```
    pub fn center(&self, tpl: &Template) -> (i32, i32) {
        (
            self.x + tpl.width as i32 / 2,
            self.y + tpl.height as i32 / 2,
        )
    }
}

/// 可插拔的匹配器。实现 [`Matcher::find`] 即可;区域查找 [`find_in`] 与多结果
/// [`find_all`] 有基于裁剪的默认实现,追求性能者可像 [`RgbMatcher`] 那样覆写。
///
/// [`find_in`]: Matcher::find_in
/// [`find_all`]: Matcher::find_all
pub trait Matcher {
    /// 整帧中找第一个匹配(最上、最左)。
    ///
    /// ⚠️ "第一个"不等于"唯一一个":命中不止一处时这里只给出扫描序 `(y, x)` 里最先
    /// 的那个,不会提示还有第二处。要判唯一请用 [`Matcher::find_all`] 并取 `max = 2`。
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

    /// 在区域内找全部不重叠匹配,按 `(y, x)` 升序返回(从上到下、从左到右),
    /// 至多 `max` 个(`max=0` 表示不限)。默认实现退化为单个。
    ///
    /// "不重叠"的判定:两个命中若在 x 方向相差小于模板宽度**且** y 方向相差小于
    /// 模板高度,算同一个目标,**保留 (y,x) 序里先出现的那个**(即最靠上、最靠左的)。
    /// 模板带掩码时宽高取可见区外接框,见 [`RgbMatcher`]。
    ///
    /// 实现方应当让 `max` **真的限制工作量**(攒够就停止扫描),而不是扫完再截断 ——
    /// [`RgbMatcher`] 就是这么做的,所以 `max = 2` 是判"命中是否唯一"的廉价守卫
    /// (1920x1200 有纹理帧 + 48x48 模板实测 `--release` ~3.3ms 串行 / ~2.2ms parallel,
    /// 与再搜一遍同量级)。用**默认实现**
    /// 的匹配器(如 `CorrMatcher`,feature `match-corr`)只会返回 1 个,拿它当守卫会
    /// 恒判"唯一"。
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
///
/// 预筛用的锚点在**模板构造时**就算好并存进 [`Template`],其字段说明在那里。它们
/// 全是模板里未被掩的真实像素,按鉴别力降序排列,所以"锚点全过"始终是"整窗匹配"
/// 的必要条件 —— 只可能白比几次,不可能漏掉真匹配。它们与容差无关,改 `tolerance` 不需要重算。
///
/// 为什么要把锚点排好、并且互相不同色:一次全屏查找的成本 ≈
/// `扫描位置数 × 预筛取数` + `活过预筛的错候选数 × 整窗核对`。前者是**内存取数**
/// 约束的(每个锚点落在屏幕不同行,基本等于一次 cache miss),所以靠"最高的那一枪
/// 就把绝大多数错位置拒掉"来降;后者是大容差下的真正大头 —— 老的"中心+四角"在
/// 横幅类模板上五个点读的是同一个底色,等于只有 1 个锚点在工作。
///
/// [`find_all`](Matcher::find_all) 按 `(y, x)` 升序返回,重叠抑制是**按行增量**做的
/// (代价与扫描同量级,不会在平坦画面上退化成二次方),并且 `max` 一到就**当场停止扫描**
/// —— 不必扫完当前行、更不必扫完后面的行,所以 `find_all(tpl, 2)` 是判"命中是否唯一"
/// 的划算守卫。[`find`](Matcher::find) 只给最上最左的一个,命中不止一处时不告知。
///
/// 平坦画面(纯色壁纸上裁出的纯色模板)是最坏情况:1920x1200 纯色帧 + 8x8 纯色模板
/// 有 36000 个命中,`--release` 实测 `max=0` ~9.9ms、`max=20` ~6µs、`max=1` ~0.7µs。
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
        let mut out: Vec<Match> = Vec::new();
        let mut keep = RowDedup::new(s.vbw, s.vbh, s.x_lo, s.x_hi);

        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            // 先**串行流式**扫一个去重窗口高度那么多行,再决定要不要上并行:
            // "攒够 max 就收工"这件事天生是顺序的(第 y 行的抑制取决于前面已经
            // 留了谁),而行块并行为凑这个早停得把整个块扫完。平坦画面上前导这一
            // 段几乎立刻就够数(首行留几个就把整行涂满覆盖,后续行 O(1) 跳过);
            // 而"整屏都不命中"的常见场景只多花前导那点串行量。
            let total = s.y_hi - s.y_lo;
            // 前导行数 = 一个去重窗口高,但不超过总行数的 1/4、也不超过 64 行(至少 1 行)。
            let lead = s.vbh.clamp(1, (total / 4).clamp(1, 64));
            let stop = s.y_lo + lead;
            let reached = scan_streaming(&s, &mut keep, max, &mut out, s.y_lo, stop);
            if reached {
                return out;
            }

            let nthreads = rayon::current_num_threads().max(1);
            // 前导已经把覆盖图铺开,用它来判断画面性质比任何猜测都准:
            // **覆盖铺满 = 命中密到连成一片**(纯色壁纸/背包格那一类)。这种画面里
            // "被覆盖的列不必 verify"能省掉 99% 的验证工作,而这件事天生是顺序的,
            // 行块并行只会把它又变回逐候选验证 —— 实测 1920x1200 纯色帧 + 240x80
            // 模板 `max=0`(120 命中):串行流式 ~6.5~7.5ms,而只做到"块"粒度早停的
            // 行块并行(含块级跳过)要 ~7.3s,`max=20` 也要 ~1s。差三个数量级,
            // 因为块内那几百万个候选全都得 verify 一遍。所以这里直接
            // 放弃并行把剩下的行流式扫完。反过来,稀疏/无命中的画面覆盖铺不满,仍走行块
            // 并行(1920x1200 有纹理帧 + 48x48 模板、15 次取中位:要扫完整帧才收工的
            // 场景 ~4.7ms 串行 → ~3.1ms 并行,两处命中提前收工 ~3.3ms → ~2.2ms)。
            if keep.row_fully_covered() {
                scan_streaming(&s, &mut keep, max, &mut out, stop, s.y_hi);
                return out;
            }
            // 按"行块"并行扫描、块内串行去重:去重必须严格按 (y,x) 顺序增量进行
            // (第 y 行只可能和 y 方向差 < h 的已保留项冲突),把它放在并行 collect
            // 之后就没法提前停了。块大小对齐核数,兼顾并行度与 `max` 的早停。
            let per = total.div_ceil(nthreads * 8).max(1);
            let mut y = stop;
            while y < s.y_hi {
                let y1 = (y + per).min(s.y_hi);
                // 覆盖图已经盖满候选列、且本块内不发生淘汰 → 整块一行都不扫。
                keep.advance_to(y);
                if keep.block_skippable(y1) {
                    y = y1;
                    continue;
                }
                let mut rows: Vec<(usize, Vec<usize>)> = (y..y1)
                    .into_par_iter()
                    .map(|yy| (yy, s.row_all(yy)))
                    .collect();
                // 不依赖 rayon 的收集顺序保证:行号是显式带出来的,排一次极便宜。
                rows.sort_unstable_by_key(|&(yy, _)| yy);
                for (yy, xs) in rows {
                    keep.push_row(yy, &xs, max, &mut out);
                    if out.len() >= max {
                        return out;
                    }
                }
                y = y1;
            }
        }
        #[cfg(not(feature = "parallel"))]
        {
            scan_streaming(&s, &mut keep, max, &mut out, s.y_lo, s.y_hi);
        }
        out
    }
}

/// 按 (y,x) 序**流式**扫描 `[y_from, y_to)`:边扫边增量去重,攒够 `max` 立刻返回
/// (返回值 = 是否已攒够)。
///
/// 与"先把整行候选收齐再去重"的结果逐字节相同,但省下两笔:
/// - **覆盖判定前置**:一个位置会不会被丢弃,只取决于覆盖图(前面已留下的命中),
///   与它自己匹不匹配无关 —— 所以被覆盖的列**不必 verify**。平坦画面上首行留下
///   的几个命中就把整行涂满,后面几十行等于免费。
/// - **行内早停**:`max` 一到就走,不必扫完当前行,更不必扫完后面的行。
fn scan_streaming(
    s: &Scan,
    keep: &mut RowDedup,
    max: usize,
    out: &mut Vec<Match>,
    y_from: usize,
    y_to: usize,
) -> bool {
    let mut kept_row: Vec<usize> = Vec::new();
    for y in y_from..y_to {
        keep.advance_to(y);
        // 整行都被窗口内的命中覆盖了 → 这一行不可能再产出命中,一行都不用扫。
        if keep.row_fully_covered() {
            continue;
        }
        kept_row.clear();
        let mut x = s.x_lo;
        if keep.covered_span == 0 {
            // 覆盖图还全空(尚未保留任何命中,或窗口刚清空):这一行不必逐个位置读
            // 覆盖图 —— 有纹理画面上"命中很稀疏"是常态,那 2M 次数组读是白花的。
            // 一旦出现首个命中就立刻切回下面的通用循环。
            while x < s.x_hi {
                if s.candidate_at(y, x) {
                    keep.keep(y, x, &mut kept_row, out);
                    if out.len() >= max {
                        return true;
                    }
                    x += 1;
                    break;
                }
                x += 1;
            }
        }
        while x < s.x_hi {
            if !keep.is_covered(x) && s.candidate_at(y, x) {
                keep.keep(y, x, &mut kept_row, out);
                if out.len() >= max {
                    return true;
                }
            }
            x += 1;
        }
        keep.finish_row(y, std::mem::take(&mut kept_row));
    }
    false
}

/// 按行增量去重器:候选**必须按 (y,x) 升序逐行喂入**。
///
/// 老实现是"每个候选和全部已保留结果线性比一遍",在平坦画面(纯色壁纸/任务栏
/// 裁出来的模板)上候选数与命中数同量级放大到几万,去重直接变成二次方 —— 1920x1200
/// 纯色帧 + 8x8 纯色模板实测 23.5 s。这里改成行方向滑动窗口 + 每列覆盖计数:
///
/// - 处理第 y 行时,只有行号落在 `[y-h+1, y]` 的已保留项可能与候选在 y 方向重叠,
///   窗口外的在 pop 时将自己那几列的计数减回去;
/// - 窗口内的保留项把自己在 x 方向的冲突区间 `[x-w+1, x+w-1]` 记进 `covered`,
///   于是单个候选的判定退化为一次数组读。
///
/// 总代价 O(候选数 + 保留数 × w),而 `保留数 × w ≤ 帧宽 × 帧高 / 模板高`,即与
/// 逐像素扫描同量级。实测 1920x1200 纯色帧 + 8x8 纯色模板(36000 命中)`max=0`:
/// 23.5 s(v0.8.0)→ ~571ms 串行 / ~185ms parallel(v0.8.1)→ **~9.9ms / ~9.8ms**
/// (v0.8.2,串行流式 + 并行前导流式;这四个数字是同一台机器上同一场景只跑一次的
/// 冷启动值,1080p 稳态中位分别是 461/138.2ms → 8.2/8.7ms)。
///
/// 串行扫描走 [`scan_streaming`]:覆盖判定在 verify **之前**问,整行被覆盖就一行不扫,
/// 攒够 `max` 当场收工 —— 这三件事把"行"粒度的早停细化到了"列"粒度。
struct RowDedup {
    /// 冲突判定用的模板宽/高(带掩码时为可见区外接框)。
    w: usize,
    h: usize,
    x_lo: usize,
    x_hi: usize,
    /// 下标 `x - x_lo`:当前窗口内有多少个已保留项在 x 方向覆盖这一列。0 = 不冲突。
    covered: Vec<u32>,
    /// `covered` 里非零元素的个数(增量维护),用来 O(1) 判断"整行都被覆盖了"。
    covered_span: usize,
    /// 滑动窗口:`(行号, 该行保留的 x)`,按行号升序,过期从队首弹出。
    window: VecDeque<(usize, Vec<usize>)>,
}

impl RowDedup {
    fn new(w: usize, h: usize, x_lo: usize, x_hi: usize) -> Self {
        RowDedup {
            w,
            h,
            x_lo,
            x_hi,
            covered: vec![0u32; x_hi - x_lo],
            covered_span: 0,
            window: VecDeque::new(),
        }
    }

    /// 推进到第 `y` 行:滑出 y 方向窗口(行号差 `>= h`)的保留行,撤销它们的覆盖标记。
    ///
    /// 减法用 `saturating_sub`:调用方必须按 (y,x) 升序喂入(下面的 `debug_assert`),
    /// 万一将来有人乱序喂,普通 `y - fy` 在 release 下会回绕成一个巨大值,把整个窗口
    /// 当成"已滑出"清空 —— 结果只是不去重、多给几个重叠命中,却很难查;饱和减则退成
    /// 保守的"不出队",至少语义还是对的。
    fn advance_to(&mut self, y: usize) {
        if let Some(&(last_y, _)) = self.window.back() {
            debug_assert!(
                last_y <= y,
                "RowDedup 必须按 (y,x) 升序喂入:窗口里第 {last_y} 行比当前行 {y} 还晚"
            );
        }
        while matches!(self.window.front(), Some(&(fy, _)) if y.saturating_sub(fy) >= self.h) {
            if let Some((_, kept)) = self.window.pop_front() {
                for x in kept {
                    self.shift(x, false);
                }
            }
        }
    }

    /// 第 `x` 列是否被窗口内的保留项覆盖。一次数组读,**可以在 verify 之前问** ——
    /// 一个位置会不会被丢弃只取决于这里,与它自己匹不匹配无关。
    #[inline]
    fn is_covered(&self, x: usize) -> bool {
        self.covered[x - self.x_lo] != 0
    }

    /// 候选列范围内**每一列**都被覆盖了:这一行不可能再产出命中,整行可以跳过。
    #[inline]
    fn row_fully_covered(&self) -> bool {
        self.covered_span == self.covered.len()
    }

    /// `[y, y1)` 这段行能否整块不扫:覆盖已铺满所有候选列,**且**本块内不会发生
    /// 窗口淘汰(淘汰会让覆盖收缩,那样就可能漏掉本该保留的命中)。
    ///
    /// 并行分支的行块扫描看不见覆盖图 —— 它得先把整块候选并行收齐才轮到去重 ——
    /// 这个判据就是给它用的:平坦画面上"一行留下的命中盖住后面几十行"正是常态。
    /// 调用前必须已经 `advance_to(y)`。
    #[cfg(feature = "parallel")]
    fn block_skippable(&self, y1: usize) -> bool {
        if !self.row_fully_covered() {
            return false;
        }
        match self.window.front() {
            Some(&(fy, _)) => (y1 - 1).saturating_sub(fy) < self.h,
            // 覆盖铺满却窗口为空:不该发生,保守当作不可跳过。
            None => false,
        }
    }

    /// 确认保留第 `y` 行的 `x`:立刻登记覆盖(同行之后的列因此免验)并产出命中。
    fn keep(&mut self, y: usize, x: usize, kept_row: &mut Vec<usize>, out: &mut Vec<Match>) {
        self.shift(x, true);
        kept_row.push(x);
        out.push(Match {
            x: x as i32,
            y: y as i32,
            score: 1.0,
        });
    }

    /// 第 `y` 行结束:把该行保留的 x 并入滑动窗口(空行不占窗口位)。
    fn finish_row(&mut self, y: usize, kept: Vec<usize>) {
        if !kept.is_empty() {
            self.window.push_back((y, kept));
        }
    }

    /// 喂入第 `y` 行的全部候选 x(升序),把通过去重的命中按序追加进 `out`,
    /// 累计到 `max` 个即停止接受。
    ///
    /// 只有并行分支的行块回灌路径用它(串行走 [`scan_streaming`],不必先把整行
    /// 候选收齐);测试拿它当朴素参照的另一半,所以 `cfg(test)` 也放行。
    #[cfg(any(test, feature = "parallel"))]
    fn push_row(&mut self, y: usize, xs: &[usize], max: usize, out: &mut Vec<Match>) {
        self.advance_to(y);
        let mut kept: Vec<usize> = Vec::new();
        for &x in xs {
            if self.is_covered(x) {
                continue;
            }
            self.keep(y, x, &mut kept, out);
            if out.len() >= max {
                break;
            }
        }
        self.finish_row(y, kept);
    }

    /// 把保留项 `x` 的冲突区间 `[x-w+1, x+w-1]`(夹到候选列范围内)的覆盖计数加一或减一。
    ///
    /// 加减严格配对(每个保留项先 shift(true) 后必然 shift(false)),计数不会为负;
    /// 仍用 `saturating_sub` 兜底,避免万一失配对时是 panic 而不是几个多余候选。
    fn shift(&mut self, x: usize, add: bool) {
        let lo = x.saturating_sub(self.w - 1).max(self.x_lo);
        let hi = (x + self.w - 1).min(self.x_hi - 1);
        for slot in &mut self.covered[lo - self.x_lo..=hi - self.x_lo] {
            if add {
                if *slot == 0 {
                    self.covered_span += 1;
                }
                *slot += 1;
            } else {
                if *slot == 1 {
                    self.covered_span -= 1;
                }
                *slot = slot.saturating_sub(1);
            }
        }
    }
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
    /// 模板预置的锚点(已按分数降序)。空 = 全被掩,预筛恒过。
    anchors: &'a [Anchor],
    mask: Option<&'a [bool]>,
    vbw: usize,
    vbh: usize,
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
        let mask = tpl.mask.as_deref();
        // 重叠抑制用的宽/高与预筛锚点都在模板构造时算好了,这里只读(见 [`Template`])。
        let (vbw, vbh) = tpl.visible_box;
        Some(Scan {
            px: &frame.pixels,
            tpl: &tpl.rgb,
            sw4: frame.width * 4,
            off: frame.rgb_offsets(),
            tw,
            th,
            tw3,
            thr,
            anchors: &tpl.anchors,
            mask,
            vbw,
            vbh,
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

    /// `(y0, x0)` 处是否命中:锚点预筛通过且整窗验证通过。流式扫描的单点判定。
    #[inline(always)]
    fn candidate_at(&self, y0: usize, x0: usize) -> bool {
        self.anchor_ok(y0, x0) && self.verify(y0, x0)
    }

    /// 该行内所有匹配的左上角 x(升序)。仅并行分支需要(每行一个独立 Vec)。
    #[cfg(feature = "parallel")]
    fn row_all(&self, y0: usize) -> Vec<usize> {
        let mut v = Vec::new();
        self.row_all_into(y0, &mut v);
        v
    }

    /// 同 `Scan::row_all`,但写进调用方复用的缓冲。
    #[cfg(feature = "parallel")]
    fn row_all_into(&self, y0: usize, out: &mut Vec<usize>) {
        out.clear();
        for x0 in self.x_lo..self.x_hi {
            if self.anchor_ok(y0, x0) && self.verify(y0, x0) {
                out.push(x0);
            }
        }
    }

    /// 多锚点预筛:所有锚点的对应屏幕像素都需在容差内才继续整窗核对。
    ///
    /// `self.anchors` 已按鉴别力降序排好,配合下面的逐个 `return false`,
    /// 绝大多数错位置在第一枪就被拒 —— 这就是排序的全部意义(预筛的代价是
    /// 按位置算的内存取数,不是比较次数)。
    #[inline(always)]
    fn anchor_ok(&self, y0: usize, x0: usize) -> bool {
        let (ro, go, bo) = self.off;
        for s in self.anchors {
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
            for tx in 0..self.tw {
                // 掩码跳过
                if let Some(m) = self.mask {
                    if !m[ty * self.tw + tx] {
                        si += 4;
                        tj += 3;
                        continue;
                    }
                }
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

    /// 大模板 + 小可见区:相距 40 px 的两个目标不应被 64 px 的模板外接框吞成一个。
    #[test]
    fn find_all_with_mask_dedups_by_visible_box() {
        let (w, h) = (128usize, 96usize);
        let (tw, th) = (64usize, 64usize);
        let s = 16usize;
        let mut px = vec![0u8; w * h * 4];
        for (tx, ty) in [(10usize, 10usize), (50usize, 10usize)] {
            for y in 0..s {
                for x in 0..s {
                    let i = ((ty + y) * w + tx + x) * 4;
                    px[i] = 200;
                    px[i + 1] = (y * 5 + 2) as u8;
                    px[i + 2] = (x * 7 + 1) as u8;
                    px[i + 3] = 255;
                }
            }
        }
        let frame = Frame::bgra8(w, h, px);
        let mut rgb = vec![0u8; tw * th * 3];
        let mut mask = vec![false; tw * th];
        for y in 0..s {
            for x in 0..s {
                let i = (y * tw + x) * 3;
                rgb[i] = (x * 7 + 1) as u8;
                rgb[i + 1] = (y * 5 + 2) as u8;
                rgb[i + 2] = 200;
                mask[y * tw + x] = true;
            }
        }
        let tpl = Template::from_rgb(rgb, tw, th).with_mask(mask);
        let got = RgbMatcher::new(0).find_all(&frame, &tpl, frame.full_rect(), 0);
        assert_eq!(
            got.len(),
            2,
            "可见区只有 16 px,相距 40 px 的两个目标应各自上报"
        );
    }

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

    /// 回归:文档承诺 (y,x) 升序,而 `raw` 里存的是 `(x0, y0)`,按元组默认序排
    /// 会变成"x 优先"。A 在右上、B 在左下,必须 A 先返回。
    #[test]
    fn find_all_orders_by_y_then_x() {
        let (w, h) = (200usize, 120usize);
        let mut px = gradient(w, h);
        let tpl = paste_block(&mut px, w, h, 150, 20, 10, [10, 200, 20]);
        paste_block(&mut px, w, h, 20, 80, 10, [10, 200, 20]);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(tpl, 10, 10);
        let all = RgbMatcher::new(0).find_all(&frame, &t, frame.full_rect(), 0);
        assert_eq!(
            all.iter().map(|m| (m.x, m.y)).collect::<Vec<_>>(),
            vec![(150, 20), (20, 80)],
            "应从上到下(而非从左到右)排"
        );
    }

    /// 重叠的一大片候选里,留下的必须是 (y,x) 序里最先出现的那个,即最靠上、最靠左。
    #[test]
    fn find_all_keeps_topmost_leftmost_of_overlapping_hits() {
        let (w, h) = (64usize, 64usize);
        let mut px = gradient(w, h);
        // 19x19 纯色块 + 10x10 模板:块内 100 个位置全是候选,但两两在 x、y 方向
        // 相差都 < 10,应只留最上最左的那一个。
        paste_block(&mut px, w, h, 12, 30, 19, [10, 200, 20]);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb([10u8, 200, 20].repeat(100).to_vec(), 10, 10);
        let all = RgbMatcher::new(0).find_all(&frame, &t, frame.full_rect(), 0);
        assert_eq!(
            all.iter().map(|m| (m.x, m.y)).collect::<Vec<_>>(),
            vec![(12, 30)],
            "一块 19x19 的平坦区只应留最上最左的命中"
        );
    }

    /// 朴素参照:O(n²),语义即文档所述((y,x) 升序、保留先遇到的)。两个对拍用例共用。
    fn naive(w: usize, h: usize, cands: &[(usize, usize)]) -> Vec<(usize, usize)> {
        let mut sorted = cands.to_vec();
        sorted.sort_unstable_by_key(|&(x, y)| (y, x));
        let mut kept: Vec<(usize, usize)> = Vec::new();
        for c in sorted {
            let clash = kept.iter().any(|k| {
                (k.0 as i64 - c.0 as i64).abs() < w as i64
                    && (k.1 as i64 - c.1 as i64).abs() < h as i64
            });
            if !clash {
                kept.push(c);
            }
        }
        kept
    }

    /// 滑窗去重必须与"每个候选和全部已保留项两两比一遍"的朴素参照逐字节一致。
    #[test]
    fn row_dedup_matches_naive_reference() {
        let (x_lo, x_hi, y_hi) = (3usize, 40usize, 24usize);
        // 线性同余伪随机,固定种子,失败可复现。
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };

        for (w, h) in [(1usize, 1usize), (4, 3), (7, 9), (13, 2), (2, 17), (30, 20)] {
            for _ in 0..20 {
                // 每行随机若干个升序且互不相同的候选列。
                let mut rows: Vec<(usize, Vec<usize>)> = Vec::new();
                for y in 0..y_hi {
                    if rnd() % 3 == 0 {
                        continue; // 整行无候选(真实扫描里很常见)
                    }
                    let mut xs: Vec<usize> = (x_lo..x_hi).filter(|_| rnd() % 5 == 0).collect();
                    xs.dedup();
                    if !xs.is_empty() {
                        rows.push((y, xs));
                    }
                }
                let flat: Vec<(usize, usize)> = rows
                    .iter()
                    .flat_map(|(y, xs)| xs.iter().map(move |&x| (x, *y)))
                    .collect();

                let mut keep = RowDedup::new(w, h, x_lo, x_hi);
                let mut out: Vec<Match> = Vec::new();
                for (y, xs) in &rows {
                    keep.push_row(*y, xs, usize::MAX, &mut out);
                }
                let got: Vec<(i32, i32)> = out.iter().map(|m| (m.x, m.y)).collect();
                let want: Vec<(i32, i32)> = naive(w, h, &flat)
                    .into_iter()
                    .map(|(x, y)| (x as i32, y as i32))
                    .collect();
                assert_eq!(got, want, "w={w} h={h} 滑窗去重与朴素参照不一致");

                // 流式路径(串行分支实际用的就是它)必须与朴素参照逐字节一致:
                // 覆盖判定前置 + 整行覆盖跳过,都不许改变"留下哪一批"。
                let mut keep_s = RowDedup::new(w, h, x_lo, x_hi);
                let mut out_s: Vec<Match> = Vec::new();
                let mut kept_row: Vec<usize> = Vec::new();
                for (y, xs) in &rows {
                    keep_s.advance_to(*y);
                    if keep_s.row_fully_covered() {
                        continue;
                    }
                    kept_row.clear();
                    for x in x_lo..x_hi {
                        if keep_s.is_covered(x) || !xs.contains(&x) {
                            continue;
                        }
                        keep_s.keep(*y, x, &mut kept_row, &mut out_s);
                    }
                    keep_s.finish_row(*y, std::mem::take(&mut kept_row));
                }
                let got_s: Vec<(i32, i32)> = out_s.iter().map(|m| (m.x, m.y)).collect();
                assert_eq!(got_s, want, "w={w} h={h} 流式扫描与朴素参照不一致");
            }
        }
    }

    /// 端到端随机对拍:上面那条合成用例只喂候选坐标,碰不到真实 `scan_streaming` 里
    /// 两条只有走生产路径才会命中的分支 —— "覆盖图为空时不读覆盖图"的免读快路径,
    /// 和"攒够 `max` 当场返回"。这条直接调 `find_all`,参照是"暴力把命中扫全 →
    /// 朴素邻域抑制 → 截断到 max"。
    ///
    /// 画面只用两种值:模板色 `MARK` 与永远取不到它的噪声。命中集合因此完全由画面
    /// 决定 —— 不能假设"我贴的那几块就是全部命中",重叠的两块会额外框出一处,暴力
    /// 参照自己会把它算出来。
    #[test]
    fn find_all_end_to_end_matches_brute_force() {
        const FW: usize = 24;
        const FH: usize = 20;
        const MARK: u8 = 200;

        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };

        // 从密集到稀疏都盖到:`MARK` 概率 1/den,den 越小越密(越密越轻易触发整行跳过)。
        for (w, h) in [(1usize, 1usize), (3, 3), (2, 5), (6, 2), (5, 7), (8, 8)] {
            for den in [2usize, 5, 14, 60] {
                let mut px = vec![0u8; FW * FH * 4];
                for ch in px.chunks_mut(4) {
                    let v = if rnd() % den == 0 {
                        MARK
                    } else {
                        40 + (rnd() % 120) as u8
                    };
                    ch[0] = v;
                    ch[1] = v;
                    ch[2] = v;
                    ch[3] = 255;
                }
                let frame = Frame::rgba8(FW, FH, px);
                let tpl = Template::from_rgb(vec![MARK; w * h * 3], w, h);

                // 暴力参照:能整块框出 `MARK` 的位置,按 (y,x) 升序。
                let mut hits: Vec<(usize, usize)> = Vec::new();
                for y in 0..=(FH - h) {
                    for x in 0..=(FW - w) {
                        if (y..y + h)
                            .all(|yy| (x..x + w).all(|xx| frame.pixels[(yy * FW + xx) * 4] == MARK))
                        {
                            hits.push((x, y));
                        }
                    }
                }

                // 四种 `max` 一起测:0 = 不限,其余测的正是行内早停与当场返回。
                for max in [0usize, 1, 2, 5] {
                    let got: Vec<(i32, i32)> = RgbMatcher::new(0)
                        .find_all(&frame, &tpl, frame.full_rect(), max)
                        .iter()
                        .map(|m| (m.x, m.y))
                        .collect();
                    let mut want: Vec<(i32, i32)> = naive(w, h, &hits)
                        .into_iter()
                        .map(|(x, y)| (x as i32, y as i32))
                        .collect();
                    if max > 0 {
                        want.truncate(max);
                    }
                    assert_eq!(
                        got, want,
                        "w={w} h={h} den={den} max={max} 端到端结果与暴力参照不一致"
                    );
                }
            }
        }
    }

    /// "整行都被覆盖 → 一行都不扫"这个跳过,不许在窗口滑过之后漏掉新命中。
    /// 24x24 纯色帧 + 4x4 纯色模板:行 0 留下的 6 个命中把行 1..3 全覆盖,行 4 起
    /// 必须重新产命中 —— 正确答案是 6x6=36 个,少一个就是跳过做得太狠。
    #[test]
    fn fully_covered_rows_are_skipped_without_losing_matches() {
        let (w, h) = (24usize, 24usize);
        let frame = Frame::rgba8(w, h, vec![9u8; w * h * 4]);
        let tpl = Template::from_rgb(vec![9u8; 4 * 4 * 3], 4, 4);
        let all = RgbMatcher::new(0).find_all(&frame, &tpl, frame.full_rect(), 0);
        let want: Vec<(i32, i32)> = (0..6)
            .flat_map(|y| (0..6).map(move |x| (x * 4, y * 4)))
            .collect();
        let got: Vec<(i32, i32)> = all.iter().map(|m| (m.x, m.y)).collect();
        assert_eq!(got, want);
    }

    /// 并行分支特有的风险:`block_skippable` 允许"覆盖已铺满、且本块内不会淘汰窗口"
    /// 时整块一行都不扫。构造一个**上半无命中、下半密到连成一片**的画面:命中行相隔
    /// 恰好是模板高,块边界与命中行故意错开,于是前导之后先出现"整块可跳过",再出现
    /// "块内发生淘汰、必须回去扫"。串行/并行两种构建下都必须给出同样的 95 个命中。
    #[test]
    fn dense_lower_half_keeps_every_match_across_block_skips() {
        let (w, h) = (40usize, 512usize);
        let mut px = gradient(w, h);
        // 下半(从 203 行起,与 8 行的块边界错开)铺成纯色
        for y in 203..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                px[i] = 9;
                px[i + 1] = 9;
                px[i + 2] = 9;
                px[i + 3] = 255;
            }
        }
        let frame = Frame::rgba8(w, h, px);
        let tpl = Template::from_rgb(vec![9u8; 8 * 16 * 3], 8, 16);
        let all = RgbMatcher::new(0).find_all(&frame, &tpl, frame.full_rect(), 0);

        let want: Vec<(i32, i32)> = (0..19i32)
            .flat_map(|k| (0..5i32).map(move |j| (j * 8, 203 + k * 16)))
            .collect();
        let got: Vec<(i32, i32)> = all.iter().map(|m| (m.x, m.y)).collect();
        assert_eq!(got, want, "下半密画面应给出 19 行 x 5 列 = 95 个命中");
    }

    /// 容差 + 平色模板很容易让屏幕上有**好几处**都满足条件,而 `find` 是第一命中
    /// 即早停:它给的是"最上、最左"的那个,并**不**告诉你还有别的。这个行为本身是
    /// 确定的(不是 bug),但足够阴 —— 所以把三条性质钉死,并给出唯一性守卫的写法。
    #[test]
    fn first_hit_is_deterministic_but_not_unique() {
        let (w, h) = (600usize, 400usize);
        let mut px = vec![0u8; w * h * 4];
        let blobs = [
            (300usize, 20usize, (200u8, 100u8, 50u8)),
            (100usize, 200usize, (215u8, 110u8, 60u8)),
        ];
        for (x0, y0, c) in blobs {
            for yy in 0..20usize {
                for xx in 0..40usize {
                    let i = ((y0 + yy) * w + (x0 + xx)) * 4;
                    px[i] = c.0;
                    px[i + 1] = c.1;
                    px[i + 2] = c.2;
                    px[i + 3] = 255;
                }
            }
        }
        let frame = Frame::rgba8(w, h, px);
        let mut tpl_rgb = Vec::with_capacity(40 * 20 * 3);
        for _ in 0..40 * 20 {
            tpl_rgb.extend_from_slice(&[200, 100, 50]);
        }
        let tpl = Template::from_rgb(tpl_rgb, 40, 20);
        let m = RgbMatcher::new(30);

        let hit = m.find(&frame, &tpl).expect("应命中");
        assert_eq!((hit.x, hit.y), (300, 20), "find 给的是 (y,x) 序第一个");

        // 守卫:想知道"其实不止一处",数到 2 就停(不必全捞)。
        let two = m.find_all(&frame, &tpl, frame.full_rect(), 2);
        assert_eq!(two.len(), 2, "两处都在容差内,find_all(max=2) 应当都看到");
        assert_eq!((two[1].x, two[1].y), (100, 200));

        // 同一帧、同一模板,只换搜索区域 → 答案换人。
        let scoped = m
            .find_in(&frame, &tpl, Rect::new(0, 150, w, 250))
            .expect("区域内应命中");
        assert_eq!((scoped.x, scoped.y), (100, 200));
    }

    /// `max` 应当真正限制工作量:只要够数就不必再往后扫行。
    #[test]
    fn find_all_stops_scanning_once_max_is_met() {
        let (w, h) = (256usize, 128usize);
        let px = vec![70u8; w * h * 4];
        let frame = Frame::bgra8(w, h, px);
        // 纯色帧 + 纯色模板:每个位置都是候选,老实现会先扫完整屏再去重。
        let t = Template::from_rgb(vec![70u8; 8 * 8 * 3], 8, 8);
        let m = RgbMatcher::new(0);
        let head = m.find_all(&frame, &t, frame.full_rect(), 2);
        assert_eq!(
            head.iter().map(|k| (k.x, k.y)).collect::<Vec<_>>(),
            vec![(0, 0), (8, 0)],
            "前两个命中应是首行相隔一个模板宽的两处"
        );
        let all = m.find_all(&frame, &t, frame.full_rect(), 0);
        assert!(all.len() > head.len(), "不限 max 时应给出全部命中");
        assert_eq!(
            &all[..head.len()],
            &head[..],
            "max 只是截断前缀,结果顺序不变"
        );
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

    /// 掩码跳过后,被掩像素即使完全不同也应命中。
    #[test]
    fn mask_skips_transparent_pixels() {
        let (w, h) = (32usize, 32usize);
        let mut px = vec![0u8; w * h * 4];
        // 贴一块 8x8 区域:前 4 像素(一行)红,后 4 像素蓝
        for x in 0..8 {
            let y = 10;
            let i = (y * w + 10 + x) * 4;
            if x < 4 {
                px[i] = 255; // R
            } else {
                px[i + 2] = 255; // B
            }
            px[i + 3] = 255;
        }
        // 模板全部声明为红,但后 4 像素被掩码跳过
        let mut tpl_rgb = Vec::with_capacity(8 * 3);
        for _ in 0..8 {
            tpl_rgb.extend_from_slice(&[255, 0, 0]);
        }
        let mask: Vec<bool> = (0..8).map(|i| i < 4).collect(); // 后 4 跳过
        let t = Template::from_rgb(tpl_rgb, 8, 1).with_mask(mask);
        let frame = Frame::rgba8(w, h, px);
        let m = RgbMatcher::new(0)
            .find(&frame, &t)
            .expect("掩码后应命中(被掩像素不比较)");
        assert_eq!((m.x, m.y), (10, 10));
    }

    /// 掩码全部为 false(等效于"什么都不用比较")——锚点列表为空,verify 不检查任何像素,
    /// 当前实现下第一个候选位置即通过(因为所有条件都"跳过")。这不算 bug,
    /// 但说明"全掩模板"语义上等价于"匹配任意位置",用户应确保至少有可见像素。
    #[test]
    fn mask_all_false_matches_first_position() {
        let (w, h) = (16usize, 16usize);
        let frame = Frame::rgba8(w, h, vec![0u8; w * h * 4]);
        let t = Template::from_rgb(vec![99u8; 4 * 4 * 3], 4, 4).with_mask(vec![false; 4 * 4]);
        let m = RgbMatcher::new(0).find(&frame, &t);
        // 全掩时没有锚点可筛,verify 全跳过 → 第一个位置就通过
        assert!(m.is_some(), "全掩模板应给出命中(语义:不需要比较任何像素)");
    }

    // ==================== 预筛锚点(与 Template 里的选取配合) ====================

    /// 从帧里裁一块当模板(RGB 顺序)。裁出来的是帧上真实的纹理,比纯色块更像真场景。
    fn cut_tpl(frame: &Frame, tx: usize, ty: usize, tw: usize, th: usize) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(tw * th * 3);
        for y in ty..ty + th {
            for x in tx..tx + tw {
                let i = (y * frame.width + x) * 4;
                rgb.extend_from_slice(&[frame.pixels[i], frame.pixels[i + 1], frame.pixels[i + 2]]);
            }
        }
        rgb
    }

    /// 零漏检不变量:在真匹配处,模板里所有未被掩的像素都与帧一致,所以**任意一组**
    /// 合法锚点都必须全部通过。这里按容差逐档直接问 `candidate_at`(不走"第一命中
    /// 即停"的扫描),就是为了把"大容差下前面先撞到一个假候选"这件事与"漏检"分开。
    #[test]
    fn true_match_survives_prefilter_at_every_tolerance() {
        let (w, h) = (96usize, 72usize);
        let px = gradient(w, h);
        let frame = Frame::rgba8(w, h, px);
        let (tw, th) = (40usize, 16usize);
        for tol in [0i32, 1, 10, 25, 40, 64] {
            // 无掩码:模板就是从同一张帧上裁下来的,真位置在 (8, 30)。
            let tpl = Template::from_rgb(cut_tpl(&frame, 8, 30, tw, th), tw, th);
            let s = Scan::new(&frame, &tpl, tol, frame.full_rect()).expect("区域足够");
            assert!(s.anchor_ok(30, 8), "tol={tol} 真匹配处必须过预筛");
            assert!(s.candidate_at(30, 8), "tol={tol} 真匹配处必须命中");

            // 带掩码:同样必须命中,而且锚点一个都不能落在被掩区里。
            let mask: Vec<bool> = (0..th * tw).map(|i| i % 7 != 0).collect();
            let tmask =
                Template::from_rgb(cut_tpl(&frame, 8, 30, tw, th), tw, th).with_mask(mask.clone());
            for a in &tmask.anchors {
                assert!(
                    mask[a.sy * tw + a.sx],
                    "tol={tol} 锚点落在被掩区: {:?}",
                    (a.sx, a.sy)
                );
            }
            let s = Scan::new(&frame, &tmask, tol, frame.full_rect()).unwrap();
            assert!(s.candidate_at(30, 8), "tol={tol} 带掩码的真匹配处必须命中");
        }
    }

    /// 改动前那套"中心+四角"的取点公式,原样搬过来做参照。
    fn legacy_points(tw: usize, th: usize) -> Vec<(usize, usize)> {
        let ix = 2.min(tw / 2);
        let iy = 2.min(th / 2);
        let pts = [
            (tw / 2, th / 2),
            (ix, iy),
            (tw - 1 - ix, iy),
            (ix, th - 1 - iy),
            (tw - 1 - ix, th - 1 - iy),
        ];
        let mut out: Vec<(usize, usize)> = Vec::new();
        for p in pts {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    /// 预筛放过多少个位置(只数锚点,不 verify)。口径与 [`Scan::anchor_ok`] 一致。
    fn pass_count(
        frame: &Frame,
        anchors: &[(usize, usize, i32, i32, i32)],
        thr: i32,
        x_hi: usize,
        y_hi: usize,
    ) -> usize {
        let (ro, go, bo) = frame.rgb_offsets();
        let stride = frame.width * 4;
        let mut n = 0usize;
        for y0 in 0..y_hi {
            for x0 in 0..x_hi {
                if anchors.iter().all(|&(sx, sy, r, g, b)| {
                    let p = (y0 + sy) * stride + (x0 + sx) * 4;
                    (frame.pixels[p + ro] as i32 - r).abs() <= thr
                        && (frame.pixels[p + go] as i32 - g).abs() <= thr
                        && (frame.pixels[p + bo] as i32 - b).abs() <= thr
                }) {
                    n += 1;
                }
            }
        }
        n
    }

    /// 这一条是本轮优化的核心证据,而且不需要真机:同一张帧、同一模板、同一容差,
    /// 只把锚点集从"中心+四角"换成模板自己选出来的一组,看预筛放过的位置数。
    ///
    /// 场景就是那个让人踩坑的形状:满屏都是横幅底色,模板的中心与四角也全落在底色上。
    /// 老的那 5 个点读的是同一个颜色,所以在整片底色上全部通过 —— 预筛形同虚设,
    /// 每个位置都要跑一次整窗核对(模板越大越贵)。
    #[test]
    fn decorrelated_anchors_reject_far_more_false_positions() {
        let (fw, fh) = (240usize, 120usize);
        let bg = [30i32, 90, 160];
        let mark = [240i32, 240, 60];
        let mut px = vec![0u8; fw * fh * 4];
        for ch in px.chunks_mut(4) {
            ch[0] = bg[0] as u8;
            ch[1] = bg[1] as u8;
            ch[2] = bg[2] as u8;
            ch[3] = 255;
        }
        // 模板:底色 + 两块标记,中心/四角都在底色上(与 template.rs 的 banner_tpl 同构)。
        let (tw, th) = (60usize, 20usize);
        let mut tpl_rgb = vec![0u8; tw * th * 3];
        for ch in tpl_rgb.chunks_mut(3) {
            ch.copy_from_slice(&[bg[0] as u8, bg[1] as u8, bg[2] as u8]);
        }
        let stamp = |buf: &mut Vec<u8>, bw: usize, x0: usize, y0: usize, c: [i32; 3]| {
            for y in y0..y0 + 2 {
                for x in x0..x0 + 4 {
                    let i = (y * bw + x) * 3;
                    buf[i] = c[0] as u8;
                    buf[i + 1] = c[1] as u8;
                    buf[i + 2] = c[2] as u8;
                }
            }
        };
        stamp(&mut tpl_rgb, tw, 10, 10, mark);
        stamp(&mut tpl_rgb, tw, 44, 6, mark);
        // 把模板原样贴到帧上 (100,50),再在 (20,20) 撒几颗零星标记点。
        for y in 0..th {
            for x in 0..tw {
                let i = ((50 + y) * fw + (100 + x)) * 4;
                px[i] = tpl_rgb[(y * tw + x) * 3];
                px[i + 1] = tpl_rgb[(y * tw + x) * 3 + 1];
                px[i + 2] = tpl_rgb[(y * tw + x) * 3 + 2];
            }
        }
        for (x, y) in [(20usize, 20usize), (21, 20), (30, 90)] {
            let i = (y * fw + x) * 4;
            px[i] = mark[0] as u8;
            px[i + 1] = mark[1] as u8;
            px[i + 2] = mark[2] as u8;
        }
        let frame = Frame::rgba8(fw, fh, px);
        let tpl = Template::from_rgb(tpl_rgb, tw, th);
        let thr = 40i32;
        let (x_hi, y_hi) = (fw - tw + 1, fh - th + 1);

        let legacy: Vec<(usize, usize, i32, i32, i32)> = legacy_points(tw, th)
            .into_iter()
            .map(|(sx, sy)| {
                let i = (sy * tw + sx) * 3;
                (
                    sx,
                    sy,
                    tpl.rgb[i] as i32,
                    tpl.rgb[i + 1] as i32,
                    tpl.rgb[i + 2] as i32,
                )
            })
            .collect();
        // 先坐实前提:老的那几个点颜色全相同,等于只有 1 个锚点在工作。
        assert!(
            legacy
                .windows(2)
                .all(|w| (w[0].2, w[0].3, w[0].4) == (w[1].2, w[1].3, w[1].4)),
            "构造失效:老的参照锚点应当同色,实际 {legacy:?}"
        );
        let new: Vec<(usize, usize, i32, i32, i32)> = tpl
            .anchors
            .iter()
            .map(|a| (a.sx, a.sy, a.r, a.g, a.b))
            .collect();
        let n_legacy = pass_count(&frame, &legacy, thr, x_hi, y_hi);
        let n_new = pass_count(&frame, &new, thr, x_hi, y_hi);
        // 真匹配处仍在(否则这条测试自己就是漏检),而预筛放过的总数大幅下跌。
        assert!(n_new > 0, "新锚点集若一个都不放过,说明构造不对");
        assert!(
            n_new * 20 < n_legacy,
            "新锚点预筛放过的位置数应远低于老的 5 点同色参照,实际 {n_new} vs {n_legacy}"
        );
    }

    /// 1×1 模板:带子塌成单点,仍要能端到端命中。
    #[test]
    fn single_pixel_template_hits() {
        let (w, h) = (32usize, 24usize);
        let mut px = gradient(w, h);
        let tpl = paste_block(&mut px, w, h, 17, 11, 1, [99, 33, 77]);
        let frame = Frame::rgba8(w, h, px);
        let t = Template::from_rgb(tpl, 1, 1);
        assert_eq!(t.anchors.len(), 1, "1×1 只可能有一个锚点");
        let m = RgbMatcher::new(0).find(&frame, &t).expect("1×1 应命中");
        // 1×1 意味着"屏幕上是这个颜色的第一个位置"就算命中,不关心具体坐标。
        assert_eq!(
            t.rgb[0] as i32,
            frame.pixels[(m.y as usize * w + m.x as usize) * 4] as i32
        );
    }
}
