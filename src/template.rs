//! 模板(要去找的那张小图)。

use image::ImageReader;

use crate::Result;

/// 模板像素数上限。**40,000,000** 个像素(约 6324 x 6324)已经远超任何合理的"要去找的
/// 那张小图":真拿这么大一张去 1080p 画面上匹配,一次耗时会长到没有实用价值。
///
/// 设上限是因为 `Template::load` 面对的输入可以是**别人给的图**(下载的素材、用户
/// 拖进来的文件),而"解压炸弹"正是拿几百 KB 的 PNG 在文件头里写巨大尺寸:解码器
/// 按声明的尺寸分配,进程直接被内存打死。32 位目标上还有一层更隐蔽的风险 —— 下面
/// `with_capacity(width * height * 3)` 这类乘法在 release 下不回绕报错,容量算小了
/// 只是后续反复扩容,不会当场失败。
const MAX_TEMPLATE_PIXELS: usize = 40_000_000;

/// 一个**预筛锚点**:相对模板左上角的偏移 + 该点的期望 RGB。
///
/// 锚点总是模板里真实存在(且未被掩掉)的像素,所以"真匹配处,每个锚点都在容差
/// 内"是"整窗匹配"的**必要条件** —— 预筛只可能白比,不可能误杀。位置与颜色都和
/// 容差无关(容差只在比较那一刻用),因此一组锚点通吃所有 `tolerance`。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub(crate) sx: usize,
    pub(crate) sy: usize,
    pub(crate) r: i32,
    pub(crate) g: i32,
    pub(crate) b: i32,
}

/// 锚点数量下限。定在 3 是因为**改动前**的"中心+四角"对小模板会塌得更少:5×5 模板
/// 上那五个点全部重合,实际只剩 1 个锚点在干活(见 `matcher.rs` 的同名回归测试)。
/// 这是"凑不到目标数时至少要给几个"的硬下限,不是目标值(目标数的地板见
/// `ANCHOR_LEGACY`)。
const ANCHOR_MIN: usize = 3;
/// 锚点数量上限。预筛的代价是**按位置算的内存取数**(每个锚点落在屏幕的不同行,
/// 基本等于一次 cache miss),所以点数不是越多越好:上限只要够把错候选在头几枪内
/// 拒掉就行。实测靶子(594×136 模板 + 全屏)容差 40 下约 8% 的位置能活过老的 5 点
/// 松筛,那些整窗核对才是大头。
const ANCHOR_MAX: usize = 12;
/// 两个锚点之间至少要有这么远的 RGB 距离(单通道最大值口径),0 表示不限。
///
/// 这条治的是"5 个点读同一个颜色":横幅/标签这类模板中心与四角都落在同一片底色上,
/// 名义 5 个锚点实际只有 1 个在工作。阈值定得小(只是"别同色")是有意的 —— 锚点不能
/// 依赖容差,而大容差下"差 8"的两个点仍可能同时通过,那部分靠点数去压,不靠调大它。
const ANCHOR_COLOR_SEP: i32 = 8;
/// 打分时邻域半径(取 |dx|,|dy| <= 2 的一圈均值,跳过出界与被掩像素)。
const ANCHOR_RAD: isize = 2;
/// 每个锚点平均摊多少像素(≈64×64)。用它从面积反推想要的锚点数与网格边长,
/// 而不是用固定 64 取整 —— 固定格长会让 128×128 以下的模板只出 1~4 个点,
/// 于是"不足下限 → 退回中心+四角",整条新逻辑对小模板白做。
const ANCHOR_DENSITY: usize = 4096;
/// 老实现的名义锚点数(中心+四角),也是新锚点目标数的**地板**:
/// 新锚点虽然互不相同色,但个数少一个就少一道闸门,而 40×40 这类常见图标按面积
/// 反推只会得到 3 个。所以目标数取 `max(老个数, 面积/密度)`,大模板才往上调。
const ANCHOR_LEGACY: usize = 5;
/// 候选池的规模上限:模板再大也只在这里量级的像素里挑锚点(等间隔子采样)。
/// 锚点仍是模板真实像素,零漏检不受影响;省的是"逐像素邻域打分"的读取次数
/// (`MAX_TEMPLATE_PIXELS` 允许 4000 万像素,逐点邻域要读上十亿次,一次 `load`
/// 会变成几百毫秒)。
const ANCHOR_POOL: usize = 250_000;

/// 一张已解码为 **RGB**(每像素 3 字节)的模板,可选携带透明掩码。
///
/// `mask` 为 `Some` 时长度等于 `width * height`:
/// `true` = 参与比较,`false` = 跳过(对应原图 alpha==0 的位置)。
/// `None` = 全部像素参与比较(向后兼容)。
///
/// 构造时(以及 [`Template::with_mask`] 改掩码时)会顺手把预筛锚点算好存进来,
/// 之后每次查找直接复用。锚点全是模板里未被掩的真实像素、按鉴别力降序排列,
/// 所以"锚点全过"始终是"整窗匹配"的必要条件 —— 代价与模板面积成正比,一次而已。
///
/// ⚠️ 字段是公开的,但**内部另有两个不可见的预计算字段**,所以不能再用
/// `Template { rgb, width, height, mask }` 这种结构体字面量构造(编不过);改用
/// [`Template::from_rgb`] / [`Template::from_rgba`] / [`Template::load`]。
#[derive(Clone, Debug)]
pub struct Template {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub mask: Option<Vec<bool>>,
    /// 预筛锚点,按鉴别力**降序**(短路判定下,最强的那个必须先比)。
    /// 空 = 没有任何可用像素(全被掩),此时预筛恒过,与改动前一致。
    pub(crate) anchors: Vec<Anchor>,
    /// 未被掩像素的外接框 `(w, h)`;无掩码(或全被掩)时等于模板尺寸。
    /// 存起来是因为 [`RgbMatcher`](crate::RgbMatcher) 的重叠抑制每次查找都要用它。
    pub(crate) visible_box: (usize, usize),
}

impl Template {
    /// 从图片文件(PNG / JPEG / WebP)加载并转成 RGB。
    ///
    /// 若图片含 alpha 通道,`alpha == 0` 的像素会被标记为掩码跳过位
    /// (匹配时这些位置不比对)。
    ///
    /// 尺寸超过 4000 万像素(`MAX_TEMPLATE_PIXELS`)时返回 [`crate::Error::Image`]
    /// (变体 `Limits`)而不是继续解码,用于挡住"解压炸弹"式的超大图片。
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        // 先只读文件头拿尺寸、确认不超上限,才真的解码。直接用 `image::open` 的话,
        // 完整解码(包括那块巨大的输出缓冲)已经跑完了才报错,内存峰值并省不下来。
        // `with_guessed_format` 仍优先按扩展名推断,与原 `image::open` 行为一致。
        // `into_dimensions` 会消耗 reader(它要建解码器才能读 IHDR/SOF),所以尺寸过了
        // 阈值后重新开一次文件 —— 多一次 open 是微秒量级,换的是"不过线就不分配"。
        let path = path.as_ref();
        let (w, h) = ImageReader::open(path)?
            .with_guessed_format()?
            .into_dimensions()?;
        Self::check_template_size(w as usize, h as usize)?;
        let img = image::open(path)?;
        // 先尝试 RGBA 以检测 alpha
        if img.color().has_alpha() {
            let rgba = img.to_rgba8();
            let (width, height) = (rgba.width() as usize, rgba.height() as usize);
            let pixels = Self::check_template_size(width, height)?;
            let mut rgb = Vec::with_capacity(pixels * 3);
            let mut mask = Vec::with_capacity(pixels);
            let mut any_transparent = false;
            for px in rgba.as_raw().as_chunks::<4>().0 {
                rgb.extend_from_slice(&[px[0], px[1], px[2]]);
                let visible = px[3] != 0;
                if !visible {
                    any_transparent = true;
                }
                mask.push(visible);
            }
            Ok(Self::assemble(
                rgb,
                width,
                height,
                if any_transparent { Some(mask) } else { None },
            ))
        } else {
            let rgb_img = img.to_rgb8();
            let (width, height) = (rgb_img.width() as usize, rgb_img.height() as usize);
            Ok(Self::assemble(rgb_img.into_raw(), width, height, None))
        }
    }

    /// 载入前与解码后的尺寸校验:乘法用 `checked_mul`(理由同 [`Template::check_len`]),
    /// 溢出或超过 `MAX_TEMPLATE_PIXELS` 都返回 [`crate::Error::Image`](用 `Limits`
    /// 变体,与 image 自己因尺寸限制拒解时的口径一致)。
    ///
    /// 返回校验过的像素数:后面把它再乘 3 / 4 当容量用时,不需要再防一次回绕。
    ///
    /// 单独拆出来是因为 `load` 需要真图片才能触发这条分支,而测试不想为了验证一个
    /// 上限去生成一张 4000 万像素的 PNG。
    fn check_template_size(width: usize, height: usize) -> Result<usize> {
        let over = || {
            crate::Error::Image(image::ImageError::Limits(
                image::error::LimitError::from_kind(image::error::LimitErrorKind::DimensionError),
            ))
        };
        let pixels = width.checked_mul(height).ok_or_else(over)?;
        if pixels > MAX_TEMPLATE_PIXELS {
            return Err(over());
        }
        Ok(pixels)
    }

    /// 尺寸断言的公共实现:元素个数必须等于 `width * height * bpp`。
    ///
    /// 之所以不直接写 `assert_eq!(len, width * height * bpp)`:release 构建默认关掉
    /// 溢出检查,荒谬尺寸(比如调用方把像素数误当宽度传进来)会让乘法**回绕**成一个
    /// 恰好等于 `len` 的值,断言形同虚设,错误拖到后面按下标取像素才暴露。
    #[inline]
    fn check_len(len: usize, width: usize, height: usize, bpp: usize, what: &str) {
        let want = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(bpp))
            .unwrap_or_else(|| panic!("{what} 尺寸溢出: {width}x{height}x{bpp}"));
        assert_eq!(len, want, "{what} 元素个数与尺寸不符");
    }

    /// 从已有 RGB 字节构造(无掩码,全部像素参与比较)。
    pub fn from_rgb(rgb: Vec<u8>, width: usize, height: usize) -> Self {
        Self::check_len(rgb.len(), width, height, 3, "template");
        Self::assemble(rgb, width, height, None)
    }

    /// 从 RGBA 字节构造,`alpha == 0` 的像素自动变为掩码跳过位。
    ///
    /// `rgba.len()` 必须等于 `width * height * 4`。
    pub fn from_rgba(rgba: Vec<u8>, width: usize, height: usize) -> Self {
        Self::check_len(rgba.len(), width, height, 4, "rgba");
        let mut rgb = Vec::with_capacity(width * height * 3);
        let mut mask = Vec::with_capacity(width * height);
        let mut any_transparent = false;
        for px in rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[px[0], px[1], px[2]]);
            let visible = px[3] != 0;
            if !visible {
                any_transparent = true;
            }
            mask.push(visible);
        }
        Self::assemble(
            rgb,
            width,
            height,
            if any_transparent { Some(mask) } else { None },
        )
    }

    /// 手动指定掩码:`mask[i] == true` 的像素参与比较,`false` 跳过。
    ///
    /// `mask.len()` 必须等于 `width * height`。
    ///
    /// ★ 这里会**重算**锚点与可见外接框:锚点必须落在未被掩的像素上,否则它拿一个
    /// "已经不参与比较的像素的颜色"去要求屏幕,就是真的漏检。构造完再 `with_mask`
    /// 是公开写法(`Template::from_rgb(...).with_mask(...)`),不是边角路径。
    pub fn with_mask(mut self, mask: Vec<bool>) -> Self {
        Self::check_len(mask.len(), self.width, self.height, 1, "mask");
        let bounds = Self::visible_bounds(self.width, self.height, Some(&mask));
        self.visible_box = match bounds {
            Some((_, _, w, h)) => (w, h),
            None => (self.width, self.height),
        };
        self.anchors = Self::build_anchors(&self.rgb, self.width, self.height, Some(&mask), bounds);
        self.mask = Some(mask);
        self
    }

    /// 所有公开构造出口的收口:顺手把可见外接框与预筛锚点算好。
    fn assemble(rgb: Vec<u8>, width: usize, height: usize, mask: Option<Vec<bool>>) -> Self {
        let bounds = Self::visible_bounds(width, height, mask.as_deref());
        let visible_box = match bounds {
            Some((_, _, w, h)) => (w, h),
            // 全被掩:退成整张尺寸(与改动前 `Scan::new` 里的写法逐字相同,重叠
            // 抑制的行为不能变)。
            None => (width, height),
        };
        let anchors = Self::build_anchors(&rgb, width, height, mask.as_deref(), bounds);
        Template {
            rgb,
            width,
            height,
            mask,
            anchors,
            visible_box,
        }
    }

    /// 未被掩像素的外接框 `(x0, y0, w, h)`;无掩码时为整张,全被掩(或零尺寸)时 `None`。
    fn visible_bounds(
        width: usize,
        height: usize,
        mask: Option<&[bool]>,
    ) -> Option<(usize, usize, usize, usize)> {
        // 注意 `None`(全部参与比较)与 `Some`(全被掩)是两件事:前者是"整张都可见",
        // 用 `?` 把它当成"无可见像素"会让**所有无掩码模板一个锚点都拿不到**。
        let Some(m) = mask else {
            return if width == 0 || height == 0 {
                None
            } else {
                Some((0, 0, width, height))
            };
        };
        let (mut x0, mut y0) = (usize::MAX, usize::MAX);
        let (mut x1, mut y1) = (0usize, 0usize);
        for y in 0..height {
            for x in 0..width {
                if m[y * width + x] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 == usize::MAX {
            None
        } else {
            Some((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
        }
    }

    /// 算预筛锚点:打分(局部对比度 + 颜色稀有度)→ 网格保散布 → 按分数降序。
    ///
    /// 只看模板自己,不读屏幕帧 —— 所以一组锚点通吃所有容差、也通吃所有画面。
    /// 全程无随机:分格、打分并列都按线性下标(即 `(y, x)` 升序)打破,同一张模板
    /// 永远得到同一组锚点。
    fn build_anchors(
        rgb: &[u8],
        width: usize,
        height: usize,
        mask: Option<&[bool]>,
        bounds: Option<(usize, usize, usize, usize)>,
    ) -> Vec<Anchor> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        // 没有可见像素就没有锚点可用:返回空表,预筛恒过。这不是漏 —— 全掩模板
        // 的语义就是"不需要比较任何像素",改动前也是同样结果(见同名测试)。
        let Some((bx, by, bw, bh)) = bounds else {
            return Vec::new();
        };
        let visible = |x: usize, y: usize| -> bool {
            match mask {
                None => true,
                Some(m) => m[y * width + x],
            }
        };
        // 内缩带宽:离模板边缘至少 `pad` 像素,躲开抗锯齿边。分轴算,不要把细长条
        // (100×3)的 x 边距被 y 拖着一起压成 0。`min` 保证带**永不为空**:老写法
        // `2.min(dim/2)` 在 dim=5 时让中心与四角全塌成同一点(改动前 5×5 只剩 1 个
        // 锚点),小尺寸的边距直接让步到 1,先保住点数。
        let pad = if width.max(height) <= 8 { 1 } else { 2 };
        let x_lo = pad.min((width - 1) / 2);
        let x_hi = width - 1 - x_lo;
        let y_lo = pad.min((height - 1) / 2);
        let y_hi = height - 1 - y_lo;
        // 候选池等间隔子采样到 ANCHOR_POOL 以内(理由见该常量注释)。起点锚定在带的
        // 左上角,保证采样位置确定。
        let band_n = (x_hi - x_lo + 1) * (y_hi - y_lo + 1);
        let mut step = 1usize;
        while band_n / step.saturating_mul(step) > ANCHOR_POOL {
            step += 1;
        }
        // 粗色直方图:每通道量化到 4 bit。★ 只统计池内**未被掩**的像素 —— 被掩处
        // 的 RGB 是 png 里原样存着的残留值(通常是 0,0,0),混进直方图会凭空造出一
        // 个巨大的黑桶,让模板里真正偏暗的边缘点被判成"常见色、低分"。
        let mut hist = vec![0u32; 1 << 12];
        let mut pool: Vec<usize> = Vec::new();
        for y in (y_lo..=y_hi).step_by(step) {
            for x in (x_lo..=x_hi).step_by(step) {
                if !visible(x, y) {
                    continue;
                }
                pool.push(y * width + x);
                hist[Self::color_bucket(rgb, y * width + x)] += 1;
            }
        }
        // 两个原始量:邻域对比度(越大越像压在边/花纹上)与颜色稀有度(-log 频次)。
        // 各自归一到 0..=1 后等权相加。
        let mut raw: Vec<(usize, f64, f64)> = Vec::with_capacity(pool.len());
        let (mut cmin, mut cmax) = (f64::MAX, f64::MIN);
        let (mut rmin, mut rmax) = (f64::MAX, f64::MIN);
        for &idx in &pool {
            let x = idx % width;
            let y = idx / width;
            let o = idx * 3;
            let (r, g, b) = (rgb[o] as i32, rgb[o + 1] as i32, rgb[o + 2] as i32);
            let (mut sum, mut n) = (0u64, 0u64);
            for dy in -ANCHOR_RAD..=ANCHOR_RAD {
                let ny = y as isize + dy;
                if ny < 0 || ny as usize >= height {
                    continue;
                }
                for dx in -ANCHOR_RAD..=ANCHOR_RAD {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as isize + dx;
                    if nx < 0 || nx as usize >= width {
                        continue;
                    }
                    let (nx, ny) = (nx as usize, ny as usize);
                    if !visible(nx, ny) {
                        continue;
                    }
                    let q = (ny * width + nx) * 3;
                    sum += ((rgb[q] as i32 - r).abs()
                        + (rgb[q + 1] as i32 - g).abs()
                        + (rgb[q + 2] as i32 - b).abs()) as u64;
                    n += 1;
                }
            }
            // 邻域全被掩/全出界 → 对比度记 0,不污染归一化区间。
            let contrast = if n == 0 { 0.0 } else { sum as f64 / n as f64 };
            // 池内自己那一格必然有它,计数 >= 1,`ln` 不会碰到 0。
            let rarity = -((hist[Self::color_bucket(rgb, idx)] as f64) + 1.0).ln();
            cmin = cmin.min(contrast);
            cmax = cmax.max(contrast);
            rmin = rmin.min(rarity);
            rmax = rmax.max(rarity);
            raw.push((idx, contrast, rarity));
        }
        let norm = |v: f64, lo: f64, hi: f64| -> f64 {
            // 整张模板一个量都不变(纯色小图标)时该项对排序无贡献,取 0 而非 NaN。
            if hi - lo < 1e-9 {
                0.0
            } else {
                (v - lo) / (hi - lo)
            }
        };
        let mut cands: Vec<(usize, f64)> = raw
            .iter()
            .map(|&(idx, c, ra)| (idx, 0.5 * norm(c, cmin, cmax) + 0.5 * norm(ra, rmin, rmax)))
            .collect();
        // 分数降序,并列按线性下标(= (y,x) 升序)。用 `total_cmp` 而不是 `partial_cmp`
        // 是为了让排序在全 NaN 之类的情形下也全序、可复现。
        cands.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

        // 想要的点数由**可见外接框面积**反推:内容只占大模板一角时,按整张面积定
        // 点数会摊到一堆没有内容的格子里,凑不够数。地板是老的 5 个(见 ANCHOR_LEGACY)。
        let area = bw * bh;
        let desired = (area / ANCHOR_DENSITY).clamp(ANCHOR_LEGACY, ANCHOR_MAX);
        // 网格边长 = ceil(sqrt(每个点应占的面积)),上限 64。同样不用固定 64:
        // 固定格长在 100×100 这类尺寸上只切出 4 格,直接退化成"补回中心+四角"。
        let cell = {
            let per = (area / desired).max(1);
            let mut c = 1usize;
            while c * c < per {
                c += 1;
            }
            c.min(64)
        };
        let gw = bw.div_ceil(cell);
        let cell_of = |x: usize, y: usize| -> usize { ((y - by) / cell) * gw + ((x - bx) / cell) };
        let mut used_cell = vec![false; gw * bh.div_ceil(cell)];
        let mut taken = vec![false; cands.len()];
        let mut chosen: Vec<Anchor> = Vec::with_capacity(desired);
        // 三轮放宽阶梯:先要"每格至多一个 + 锚点间不同色",不够就放颜色,还不足
        // 下限再放格限额。老写法里"补回中心+四角"的兜底不再需要 —— 候选池本来就
        // 只装合法像素,取前几名自动是可用点。
        //
        // `chosen` 走参数而不是闭包捕获:捕获式可变借用在两次 `fill` 之间读不到
        // `chosen.len()`,而"上一轮够不够"正是这三段的唯一区别。
        let mut fill = |chosen: &mut Vec<Anchor>, sep: i32, per_cell: bool, stop: usize| {
            if chosen.len() >= stop {
                return;
            }
            for (i, &(idx, _)) in cands.iter().enumerate() {
                if taken[i] {
                    continue;
                }
                let x = idx % width;
                let y = idx / width;
                let ci = cell_of(x, y);
                if per_cell && used_cell[ci] {
                    continue;
                }
                let o = idx * 3;
                let (r, g, b) = (rgb[o] as i32, rgb[o + 1] as i32, rgb[o + 2] as i32);
                if sep > 0
                    && chosen
                        .iter()
                        .any(|a| (a.r - r).abs().max((a.g - g).abs()).max((a.b - b).abs()) < sep)
                {
                    continue;
                }
                chosen.push(Anchor {
                    sx: x,
                    sy: y,
                    r,
                    g,
                    b,
                });
                taken[i] = true;
                used_cell[ci] = true;
                if chosen.len() >= stop {
                    break;
                }
            }
        };
        fill(&mut chosen, ANCHOR_COLOR_SEP, true, desired);
        if chosen.len() < desired {
            fill(&mut chosen, 0, true, desired);
        }
        if chosen.len() < ANCHOR_MIN {
            fill(&mut chosen, 0, false, ANCHOR_MIN);
        }
        chosen
    }

    /// 12 bit 粗色桶(每通道量化到 4 bit)。
    fn color_bucket(rgb: &[u8], pixel_index: usize) -> usize {
        let o = pixel_index * 3;
        ((rgb[o] >> 4) as usize) << 8
            | ((rgb[o + 1] >> 4) as usize) << 4
            | (rgb[o + 2] >> 4) as usize
    }

    /// 内容指纹(尺寸 + 像素哈希 + 掩码指纹)。用于匹配器缓存预计算结果,内容变则失效。
    pub fn content_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.width.hash(&mut h);
        self.height.hash(&mut h);
        self.rgb.hash(&mut h);
        self.mask.hash(&mut h);
        h.finish()
    }

    /// 转成单通道灰度(Rec.601 加权),供 ZNCC 等匹配器使用。
    ///
    /// 注意:ZNCC 当前**不使用掩码**,所有像素(包括被掩掉的)均参与灰度转换。
    pub fn to_gray(&self) -> Vec<u8> {
        // 只是容量提示,长度仍由循环决定;用 `saturating_mul` 是为了在荒谬尺寸下
        // 不把乘法回绕成一个小数字(那只会多几次扩容,但没必要留着)。
        let mut gray = Vec::with_capacity(self.width.saturating_mul(self.height));
        for px in self.rgb.as_chunks::<3>().0 {
            let r = px[0] as u32;
            let g = px[1] as u32;
            let b = px[2] as u32;
            gray.push(((r * 299 + g * 587 + b * 114) / 1000) as u8);
        }
        gray
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rgb_no_mask() {
        let t = Template::from_rgb(vec![128u8; 4 * 4 * 3], 4, 4);
        assert!(t.mask.is_none());
    }

    #[test]
    fn from_rgba_detects_transparent() {
        // 4x4, (1,1) alpha=0, 其余 alpha=255
        let (w, h) = (4usize, 4usize);
        let mut rgba = vec![200u8; w * h * 4];
        rgba[(w + 1) * 4 + 3] = 0; // (1,1) 透明
        let t = Template::from_rgba(rgba, w, h);
        let mask = t.mask.as_ref().expect("应检测到透明像素");
        assert!(!mask[w + 1], "(1,1) 应被标记为跳过");
        assert!(mask[0], "(0,0) 应参与比较");
    }

    #[test]
    fn with_mask_overrides() {
        let t =
            Template::from_rgb(vec![0u8; 2 * 2 * 3], 2, 2).with_mask(vec![true, false, true, true]);
        assert!(!t.mask.as_ref().unwrap()[1]);
    }

    #[test]
    fn content_key_changes_with_mask() {
        let a = Template::from_rgb(vec![10u8; 2 * 2 * 3], 2, 2);
        let b = Template::from_rgb(vec![10u8; 2 * 2 * 3], 2, 2)
            .with_mask(vec![true, false, true, true]);
        assert_ne!(a.content_key(), b.content_key());
    }

    /// 上限本身的分档测试。不依赖真实图片:为了验证一个阈值去生成一张
    /// 4000 万像素的 PNG 既慢又占地方。
    #[test]
    fn check_template_size_tiers() {
        // 恰好在阈值上:放行
        assert_eq!(
            Template::check_template_size(8000, MAX_TEMPLATE_PIXELS / 8000).unwrap(),
            MAX_TEMPLATE_PIXELS
        );
        // 多一个像素:拒
        assert!(Template::check_template_size(8000, MAX_TEMPLATE_PIXELS / 8000 + 1).is_err());
        // 零尺寸不是错误(尺寸合法性由下游 `Scan::new` 处理,那里直接返回 None)
        assert_eq!(Template::check_template_size(0, 100).unwrap(), 0);
    }

    /// `width * height` 本身就能超出 `usize::MAX`:必须是 `Err` 而不是 panic,
    /// 也不是回绕后的那个小数字。只在 64 位目标上构造得出这种尺寸。
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn check_template_size_rejects_overflow() {
        assert!(Template::check_template_size(1 << 32, 1 << 32).is_err());
    }

    /// 临时文件路径:用进程号 + 测试名避开并发跑时互相踩。
    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pixhunt_tpl_{name}_{}.png", std::process::id()))
    }

    /// 走一遍真实的"落盘→ `Template::load`"流程,验证换成 `ImageReader` 后
    /// 常规路径没坏(包括扩展名推断格式这条)。
    #[test]
    fn load_roundtrip_reads_a_real_png() {
        let path = temp_path("roundtrip");
        let img = image::DynamicImage::new_rgb8(4, 3);
        img.save(&path).unwrap();
        let t = Template::load(&path).expect("应能载入一张普通 PNG");
        let _ = std::fs::remove_file(&path);
        assert_eq!((t.width, t.height), (4, 3));
        assert_eq!(t.rgb.len(), 4 * 3 * 3);
        assert!(t.mask.is_none());
    }

    /// "解压炸弹":拿一张真 PNG 只改掉文件头里的尺寸声明(后边仍是那 60 多字节)。
    /// `load` 必须在**解码前**就拦住,而不是先按声明尺寸分配完再报错。
    #[test]
    fn load_rejects_oversized_declared_dimensions() {
        let path = temp_path("bomb");
        let img = image::DynamicImage::new_rgb8(4, 3);
        img.save(&path).unwrap();
        // IHDR 块布局(跳过 8 字节 PNG 签名后):[8..12)=数据长度,[12..16)="IHDR",
        // [16..29)=13 字节数据(宽=[16..20) 高=[20..24) 大端,余下色深等),[29..33)=CRC。
        // CRC 盖的是 type+data 即 [12..29),改完尺寸必须重算 —— 不然先撞上的就是
        // 校验和报错,测不到尺寸上限这条分支。
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        let crc = crc32(&bytes[12..29]).to_be_bytes();
        bytes[29..33].copy_from_slice(&crc);
        std::fs::write(&path, &bytes).unwrap();

        let err = match Template::load(&path) {
            Ok(_) => panic!("100000x100000 应被上限拦住"),
            Err(e) => e,
        };
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(err, crate::Error::Image(image::ImageError::Limits(_))),
            "应报成 ImageError::Limits,实际: {err}"
        );
    }

    /// 多项式 0xEDB88320 的 CRC-32(就是 PNG/zlib 用的那个),逐位算即可:
    /// 测试里只跑几十字节,不值得为了它引一个 crc 依赖。
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    // ==================== 预筛锚点 ====================

    /// 模板底色与标记色(横幅类画面的两个代表性颜色)。
    const BG: [u8; 3] = [30, 90, 160];
    const MARK: [u8; 3] = [240, 240, 60];

    fn anchor_tups(t: &Template) -> Vec<(usize, usize, (i32, i32, i32))> {
        t.anchors
            .iter()
            .map(|a| (a.sx, a.sy, (a.r, a.g, a.b)))
            .collect()
    }

    /// 锚点的硬性合法条件:坐标在尺寸内、未被掩、颜色就是那个位置的模板像素。
    /// 后两条是"零漏检"的地基 —— 锚点只要不是模板里真实且不参与比较的像素,
    /// 预筛就可能误杀真匹配。坐标重复同理(等于少一道闸门)。
    fn assert_anchor_legal(t: &Template) {
        let mask = t.mask.as_deref();
        let mut seen: Vec<(usize, usize)> = Vec::new();
        for a in &t.anchors {
            assert!(
                a.sx < t.width && a.sy < t.height,
                "锚点坐标越界: {:?}",
                (a.sx, a.sy)
            );
            if let Some(m) = mask {
                assert!(
                    m[a.sy * t.width + a.sx],
                    "锚点落在被掩像素上: {:?}",
                    (a.sx, a.sy)
                );
            }
            let i = (a.sy * t.width + a.sx) * 3;
            assert_eq!(
                (a.r, a.g, a.b),
                (t.rgb[i] as i32, t.rgb[i + 1] as i32, t.rgb[i + 2] as i32),
                "锚点颜色与模板像素不符"
            );
            assert!(!seen.contains(&(a.sx, a.sy)), "锚点坐标重复");
            seen.push((a.sx, a.sy));
        }
    }

    /// 一张"横幅"模板:底色铺满,中间两块标记,中心与四角全落在底色上。
    fn banner_tpl() -> Template {
        let (tw, th) = (60usize, 20usize);
        let mut rgb = vec![0u8; tw * th * 3];
        for px in rgb.chunks_mut(3) {
            px.copy_from_slice(&BG);
        }
        for (x0, y0) in [(10usize, 10usize), (44usize, 6usize)] {
            for y in y0..y0 + 2 {
                for x in x0..x0 + 4 {
                    let i = (y * tw + x) * 3;
                    rgb[i..i + 3].copy_from_slice(&MARK);
                }
            }
        }
        Template::from_rgb(rgb, tw, th)
    }

    #[test]
    fn anchors_do_not_collapse_on_small_templates() {
        // 改动前:5×5 上"中心+四角"经 `2.min(dim/2)` 全部重合到 (2,2),去重后只剩
        // 1 个锚点。现在小尺寸的边距退让到 1 像素,9 个候选里至少取到下限那么多。
        let rgb: Vec<u8> = (0..25 * 3).map(|i| (i * 37 % 251) as u8).collect();
        let t = Template::from_rgb(rgb, 5, 5);
        assert_anchor_legal(&t);
        assert!(
            t.anchors.len() >= ANCHOR_MIN,
            "5×5 应至少 {} 个锚点,实际 {}",
            ANCHOR_MIN,
            t.anchors.len()
        );
    }

    #[test]
    fn anchor_count_has_legacy_floor_and_cap() {
        // 40×40:按面积反推只要 3 个,但地板是老个数 5 —— 不能比改动前少一道闸门。
        let rgb: Vec<u8> = (0..40 * 40 * 3).map(|i| (i * 7 + i / 13) as u8).collect();
        let small = Template::from_rgb(rgb, 40, 40);
        assert_anchor_legal(&small);
        assert_eq!(
            small.anchors.len(),
            ANCHOR_LEGACY,
            "常见小图标不得少于老的 5 个"
        );
        // 300×300:面积反推早已超过上限,被 ANCHOR_MAX 封住(预筛的代价按位置算,
        // 点数不是越多越好)。
        let big = Template::from_rgb(vec![7u8; 300 * 300 * 3], 300, 300);
        assert_eq!(big.anchors.len(), ANCHOR_MAX, "纯色大模板也应凑满上限");
        assert_anchor_legal(&big);
    }

    #[test]
    fn banner_anchors_are_not_all_the_same_color() {
        // 这条直接治"5 个点读同一个底色":锚点之间必须拉开颜色距离。
        let t = banner_tpl();
        assert_anchor_legal(&t);
        let colors: Vec<(i32, i32, i32)> = t.anchors.iter().map(|a| (a.r, a.g, a.b)).collect();
        let far_apart = colors.iter().enumerate().any(|(i, a)| {
            colors[..i].iter().any(|b| {
                (a.0 - b.0)
                    .abs()
                    .max((a.1 - b.1).abs())
                    .max((a.2 - b.2).abs())
                    >= ANCHOR_COLOR_SEP
            })
        });
        assert!(
            far_apart,
            "锚点之间应至少有一对颜色差 >= {ANCHOR_COLOR_SEP},实际 {colors:?}"
        );
        // 分数最高的那个必须是标记(对比度 + 稀有度双高),不是底色 —— 短路判定下
        // 它才是"第一枪"。
        let first = t.anchors.first().unwrap();
        assert_eq!(
            (first.r as u8, first.g as u8, first.b as u8),
            (MARK[0], MARK[1], MARK[2]),
            "第一枪应打在标记上"
        );
    }

    #[test]
    fn with_mask_never_keeps_anchor_on_masked_pixel() {
        // 这是真正的漏检风险:锚点在 `from_rgb` 时按无掩码选好,之后 `with_mask`
        // 把那个像素掩掉了 —— 它却还拿"已不参与比较的像素的颜色"去要求屏幕。
        let first = banner_tpl().anchors[0];
        let mut mask = vec![true; 60 * 20];
        mask[first.sy * 60 + first.sx] = false;
        let t = banner_tpl().with_mask(mask);
        assert_anchor_legal(&t);
        assert!(
            !t.anchors
                .iter()
                .any(|a| a.sx == first.sx && a.sy == first.sy),
            "被掩掉的像素不能再当锚点"
        );
    }

    #[test]
    fn anchor_scoring_ignores_masked_pixels() {
        // 被掩处的 RGB 是 png 里原样存的残留值(常常是全黑),它绝不能进色直方图:
        // 不然凭空多出一个巨大的"黑桶",把可见区里真正偏暗的边缘点判成常见色、低分。
        // 做法:可见区一模一样、只有被掩处从全黑换成全白的两张模板,锚点必须逐个相同。
        let (w, h) = (32usize, 32usize);
        let mut mask = vec![false; w * h];
        for y in 0..h {
            for x in 0..16 {
                mask[y * w + x] = true; // 只有左半参与比较
            }
        }
        let build = |ghost: [u8; 3]| {
            let mut rgb = vec![0u8; w * h * 3];
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) * 3;
                    if x < 16 {
                        let v = if (x * 3 + y * 5) % 17 == 0 {
                            4
                        } else {
                            120 + x as u8
                        };
                        rgb[i] = v;
                        rgb[i + 1] = v / 2;
                        rgb[i + 2] = (x * 8 + y) as u8;
                    } else {
                        rgb[i..i + 3].copy_from_slice(&ghost);
                    }
                }
            }
            Template::from_rgb(rgb, w, h).with_mask(mask.clone())
        };
        let dark = anchor_tups(&build([0, 0, 0]));
        let light = anchor_tups(&build([255, 255, 255]));
        assert!(!dark.is_empty());
        assert_eq!(dark, light, "被掩像素的残留颜色不该影响锚点选取");
    }

    #[test]
    fn anchors_empty_when_everything_masked() {
        let t = Template::from_rgb(vec![9u8; 4 * 4 * 3], 4, 4).with_mask(vec![false; 16]);
        assert!(t.anchors.is_empty(), "无可比较像素时不应凭空造锚点");
        // 外接框退成整张尺寸 —— 重叠抑制的行为必须与改动前逐字相同。
        assert_eq!(t.visible_box, (4, 4));
    }

    #[test]
    fn anchors_are_deterministic() {
        assert_eq!(
            anchor_tups(&banner_tpl()),
            anchor_tups(&banner_tpl()),
            "同一张模板永远得到同一组锚点(无随机)"
        );
    }

    #[test]
    fn thin_templates_stay_in_bounds() {
        // 带子塌成一行/一列时也不能空:坐标越界或 0 个锚点都是 bug。
        for (w, h) in [
            (1usize, 1usize),
            (1, 9),
            (9, 1),
            (2, 2),
            (3, 1),
            (4, 4),
            (8, 3),
        ] {
            let rgb = vec![(w * 17 + h * 29) as u8; w * h * 3];
            let t = Template::from_rgb(rgb, w, h);
            assert_anchor_legal(&t);
            assert!(!t.anchors.is_empty(), "{w}x{h} 应有至少一个锚点");
        }
    }

    #[test]
    fn big_template_subsamples_the_pool() {
        // 带子超过 ANCHOR_POOL 时走等间隔子采样(步长 > 1):锚点仍必须逐个合法,
        // 并且仍然凑满上限。
        let s = 600usize;
        let rgb: Vec<u8> = (0..s * s * 3).map(|i| (i * 31 + i / 7) as u8).collect();
        let t = Template::from_rgb(rgb, s, s);
        assert_anchor_legal(&t);
        assert_eq!(t.anchors.len(), ANCHOR_MAX);
    }

    #[test]
    fn visible_box_follows_mask() {
        let (w, h) = (32usize, 24usize);
        let mut mask = vec![false; w * h];
        for y in 4..12 {
            for x in 10..30 {
                mask[y * w + x] = true;
            }
        }
        let t = Template::from_rgb(vec![5u8; w * h * 3], w, h).with_mask(mask);
        assert_eq!(t.visible_box, (20, 8), "外接框应贴着可见像素");
        let plain = Template::from_rgb(vec![5u8; w * h * 3], w, h);
        assert_eq!(plain.visible_box, (w, h), "无掩码时等于整张");
    }
}
