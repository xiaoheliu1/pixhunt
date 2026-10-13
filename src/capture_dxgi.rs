//! Windows DXGI 桌面复制截图后端:GPU 取帧,D3D11 设备/上下文/staging 纹理全部
//! 只建一次并复用;每帧只做 AcquireNextFrame →(仅当有新帧时)CopyResource → Map →
//! 按行回拷 → Unmap → ReleaseFrame。输出 **BGRA**。
//!
//! 桌面复制固有:取帧受刷新率限制,静态桌面 `AcquireNextFrame` 返回 WAIT_TIMEOUT,
//! 表示"自上次以来画面没变"。据此 [`Capture::grab_into`] 会:不重新回拷、并返回
//! `changed = false`,让上层安全跳过搜索。

use crate::capture::Capture;
use crate::frame::Frame;
use crate::{Error, Result};

use windows::core::ComInterface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory2, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_ERROR_UNSUPPORTED, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_DESC, DXGI_OUTDUPL_FRAME_INFO,
};

/// 单次取帧的等待上限(毫秒)。超时视为"无新帧"。
const ACQUIRE_TIMEOUT_MS: u32 = 33;

struct DxgiInner {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// 留着是为了能在失效后重新 `DuplicateOutput`(重建路径)。
    output1: IDXGIOutput1,
    duplication: IDXGIOutputDuplication,
    staging: ID3D11Texture2D,
    w: usize,
    h: usize,
    have_frame: bool,
}

impl DxgiInner {
    fn try_new() -> windows::core::Result<DxgiInner> {
        unsafe {
            let factory: IDXGIFactory2 = CreateDXGIFactory1()?;
            let adapter = factory.EnumAdapters(0)?;

            let mut device_opt: Option<ID3D11Device> = None;
            let mut context_opt: Option<ID3D11DeviceContext> = None;
            // 指定 adapter 时 drivertype 必须为 UNKNOWN;开启 BGRA 支持
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                7,
                Some(&mut device_opt),
                None,
                Some(&mut context_opt),
            )?;
            let device = device_opt.ok_or_else(windows::core::Error::from_win32)?;
            let context = context_opt.ok_or_else(windows::core::Error::from_win32)?;
            let _ = D3D_DRIVER_TYPE_HARDWARE;

            let output = adapter.EnumOutputs(0)?;
            let output1: IDXGIOutput1 = output.cast()?;
            let (duplication, staging, w, h) = Self::build_duplication(&output1, &device)?;
            Ok(DxgiInner {
                device,
                context,
                output1,
                duplication,
                staging,
                w,
                h,
                have_frame: false,
            })
        }
    }

    /// 建一套(输出复制对象, staging 纹理, 宽, 高)。全部成功才返回,
    /// 因此调用方要么拿到一套完整可用的对象,要么什么都没有 —— 不会出现
    /// 半初始化的字段。
    fn build_duplication(
        output1: &IDXGIOutput1,
        device: &ID3D11Device,
    ) -> windows::core::Result<(IDXGIOutputDuplication, ID3D11Texture2D, usize, usize)> {
        unsafe {
            let duplication = output1.DuplicateOutput(device)?;

            let mut od = std::mem::zeroed::<DXGI_OUTDUPL_DESC>();
            duplication.GetDesc(&mut od);
            let w = od.ModeDesc.Width as usize;
            let h = od.ModeDesc.Height as usize;
            let fmt: DXGI_FORMAT = od.ModeDesc.Format;
            // 下面的回拷把两件事写死了:每像素 4 字节,且内存里就是 B/G/R/A 顺序。
            // 本机实测 DuplicateOutput 给的就是 `B8G8R8A8_UNORM`(值 87),但这不是
            // 保证:开启 HDR / 10bit 输出时会上报 `R16G16B16A16_FLOAT`(8 字节/像素),
            // 硬拷会把一行只读一半,得出一张噪声画面并**静默匹配失败**。在这里报错
            // 比那个结果好得多:`CaptureKind::Auto` 会退到 GDI/xcap,调用方能看见原因。
            if fmt != DXGI_FORMAT_B8G8R8A8_UNORM {
                return Err(windows::core::Error::new(
                    DXGI_ERROR_UNSUPPORTED,
                    format!(
                        "unexpected desktop duplication format {} (want B8G8R8A8_UNORM, \
                         the copy loop below assumes 4 bytes per pixel)",
                        fmt.0
                    )
                    .into(),
                ));
            }

            let mut desc = std::mem::zeroed::<D3D11_TEXTURE2D_DESC>();
            desc.Width = w as u32;
            desc.Height = h as u32;
            desc.MipLevels = 1;
            desc.ArraySize = 1;
            desc.Format = fmt;
            desc.SampleDesc = DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            };
            desc.Usage = D3D11_USAGE_STAGING;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

            let mut staging_opt: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&desc, None, Some(&mut staging_opt))?;
            let staging = staging_opt.ok_or_else(windows::core::Error::from_win32)?;

            Ok((duplication, staging, w, h))
        }
    }

    /// (重新)建立输出复制对象与 staging 纹理。
    ///
    /// 分辨率改变、锁屏/休眠唤醒、显卡设备重置都会让旧 `duplication` 失效
    /// (`AcquireNextFrame` 报 `DXGI_ERROR_ACCESS_LOST`),此时**必须重建**:
    /// 否则本后端会把持续报错当成"画面没有新帧",让调用方永远看到失效前的
    /// 旧画面。桌面尺寸变化也由这里跟上(staging 按新 `ModeDesc` 重建)。
    ///
    /// 失败时不改写任何字段,`self` 仍持有上一套对象(可继续尝试)。
    fn rebuild_duplication(&mut self) -> windows::core::Result<()> {
        let (duplication, staging, w, h) = Self::build_duplication(&self.output1, &self.device)?;
        self.duplication = duplication;
        self.staging = staging;
        self.w = w;
        self.h = h;
        self.have_frame = false;
        Ok(())
    }

    /// 抓一帧写进 `dst`。返回 `changed`:自上次以来是否收到新帧
    /// (`false` 表示画面逐像素未变,`dst` 保留上一帧内容)。
    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> {
        unsafe {
            let mut res: Option<IDXGIResource> = None;
            let mut fi = std::mem::zeroed::<DXGI_OUTDUPL_FRAME_INFO>();
            // acquire 成功就等于占用了一帧,**必须** ReleaseFrame,哪怕这一帧没带
            // 资源(res 为 None)。漏掉一次会让后续的 AcquireNextFrame 一直失败,
            // 表现与"复制对象失效"相同、却比它更难查。
            let mut acquired = false;
            let fresh =
                match self
                    .duplication
                    .AcquireNextFrame(ACQUIRE_TIMEOUT_MS, &mut fi, &mut res)
                {
                    Ok(()) => {
                        acquired = true;
                        res.is_some()
                    }
                    // WAIT_TIMEOUT 的含义是"自上次以来画面没变",属正常路径。
                    Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => false,
                    // 其余错误(ACCESS_LOST / DEVICE_REMOVED / 显示模式改变)意味着复制
                    // 对象已经失效。不重建的话,它会一直走到上面的超时分支,让调用方
                    // **永远看到失效前的旧画面**——所以这里重建并立刻再取一次。
                    // 代价:刚重建好、系统还没推新帧时,本帧可能是全零(黑),下一帧起正常。
                    Err(_) => {
                        // 失败调用是否仍会写入资源,取决于上游实现;这里先丢弃一次
                        // (释放那个 COM 引用),重试就不可能悄悄漏掉一个引用计数。
                        res = None;
                        self.rebuild_duplication().map_err(|e| {
                            Error::capture_from("dxgi duplication rebuild failed", e)
                        })?;
                        match self.duplication.AcquireNextFrame(
                            ACQUIRE_TIMEOUT_MS,
                            &mut fi,
                            &mut res,
                        ) {
                            Ok(()) => {
                                acquired = true;
                                res.is_some()
                            }
                            Err(_) => false,
                        }
                    }
                };
            if acquired {
                if let Some(r) = res.take() {
                    if let Ok(tex) = r.cast::<ID3D11Texture2D>() {
                        self.context.CopyResource(&self.staging, &tex);
                    }
                }
                let _ = self.duplication.ReleaseFrame();
            }

            // 只有收到新帧,或还没抓过任何帧时,才回拷(否则 dst 已是最新)。
            let need = fresh || !self.have_frame;
            let mut wrote = false;
            if need {
                let mut mapped = std::mem::zeroed::<D3D11_MAPPED_SUBRESOURCE>();
                if self
                    .context
                    .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .is_ok()
                {
                    let row = self.w * 4;
                    // 回拷前的最后一道契约:行距不能小于一行要的字节数。带 padding
                    // 的行距(大于 `row`)是安全的,小于就是越界读 —— 今天由上面的格式
                    // 检查间接保证,但 Map 的结果属于驱动,不该拿安全当免费。
                    if (mapped.RowPitch as usize) < row {
                        self.context.Unmap(&self.staging, 0);
                        return Err(Error::capture(format!(
                            "dxgi staging RowPitch {} < row bytes {row}",
                            mapped.RowPitch
                        )));
                    }
                    dst.prepare_bgra(self.w, self.h);
                    let src = mapped.pData as *const u8;
                    let dst_ptr = dst.pixels.as_mut_ptr();
                    for y in 0..self.h {
                        std::ptr::copy_nonoverlapping(
                            src.add(y * mapped.RowPitch as usize),
                            dst_ptr.add(y * row),
                            row,
                        );
                    }
                    self.context.Unmap(&self.staging, 0);
                    self.have_frame = true;
                    wrote = true;
                }
            }
            // 只要动过 `dst` 就不能回报"画面未变":重建后、系统还没推新帧的那一帧里,
            // `fresh` 是 false 但我们仍然把新分辨率的 staging(可能全黑)写进了 `dst`。
            // 上层按"未变"走缓存的话,会拿到上一个分辨率下算出的命中坐标去点鼠标。
            // 首帧同理(没有"上次"可比,说"变了"最保守)。
            Ok(fresh || wrote)
        }
    }
}

/// [`Capture`] 的 DXGI 桌面复制实现,产出 BGRA [`Frame`]。
pub struct DxgiCapture {
    inner: DxgiInner,
}

impl DxgiCapture {
    /// 尝试建立桌面复制;无 D3D / RDP / 锁屏等场景返回 `None`。
    ///
    /// **选屏是按 `EnumAdapters(0)` + `EnumOutputs(0)` 定的**,即"第一个显卡的第一个
    /// 输出"。单显卡机器上它就是主屏;多显卡(核显 + 独显)时这个顺序由驱动决定,
    /// **不保证是主显示器** —— 表现是画面能抓到、坐标也对,只是抓的是另一块屏。
    /// 要按坐标选屏请用 `XCapCapture::from_point`(需 feature `capture-xcap`),要盯
    /// 某个窗口请用 `WindowCapture`(需 feature `capture-window`)。
    pub fn new_primary() -> Option<Self> {
        DxgiInner::try_new().ok().map(|inner| DxgiCapture { inner })
    }
}

impl Capture for DxgiCapture {
    fn grab(&mut self) -> Result<Frame> {
        let mut frame = Frame::bgra8(0, 0, Vec::new());
        self.inner.grab_into(&mut frame)?;
        if frame.pixels.is_empty() {
            return Err(Error::capture("dxgi returned empty frame"));
        }
        Ok(frame)
    }

    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> {
        let changed = self.inner.grab_into(dst)?;
        if dst.pixels.is_empty() {
            return Err(Error::capture("dxgi returned empty frame"));
        }
        Ok(changed)
    }

    fn backend(&self) -> &'static str {
        "dxgi"
    }
}
