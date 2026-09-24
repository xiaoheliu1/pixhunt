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
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory2, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_OUTDUPL_DESC, DXGI_OUTDUPL_FRAME_INFO,
};

/// 单次取帧的等待上限(毫秒)。超时视为"无新帧"。
const ACQUIRE_TIMEOUT_MS: u32 = 33;

struct DxgiInner {
    _device: ID3D11Device,
    context: ID3D11DeviceContext,
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
            let duplication = output1.DuplicateOutput(&device)?;

            let mut od = std::mem::zeroed::<DXGI_OUTDUPL_DESC>();
            duplication.GetDesc(&mut od);
            let w = od.ModeDesc.Width as usize;
            let h = od.ModeDesc.Height as usize;
            let fmt: DXGI_FORMAT = od.ModeDesc.Format;

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

            Ok(DxgiInner {
                _device: device,
                context,
                duplication,
                staging,
                w,
                h,
                have_frame: false,
            })
        }
    }

    /// 抓一帧写进 `dst`。返回 `changed`:自上次以来是否收到新帧
    /// (`false` 表示画面逐像素未变,`dst` 保留上一帧内容)。
    fn grab_into(&mut self, dst: &mut Frame) -> bool {
        unsafe {
            let mut res: Option<IDXGIResource> = None;
            let mut fi = std::mem::zeroed::<DXGI_OUTDUPL_FRAME_INFO>();
            let fresh =
                match self
                    .duplication
                    .AcquireNextFrame(ACQUIRE_TIMEOUT_MS, &mut fi, &mut res)
                {
                    Ok(()) => res.is_some(),
                    Err(_) => false,
                };
            if fresh {
                if let Some(r) = res.take() {
                    if let Ok(tex) = r.cast::<ID3D11Texture2D>() {
                        self.context.CopyResource(&self.staging, &tex);
                    }
                }
                let _ = self.duplication.ReleaseFrame();
            }

            // 只有收到新帧,或还没抓过任何帧时,才回拷(否则 dst 已是最新)。
            let need = fresh || !self.have_frame;
            if need {
                let mut mapped = std::mem::zeroed::<D3D11_MAPPED_SUBRESOURCE>();
                if self
                    .context
                    .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .is_ok()
                {
                    dst.prepare_bgra(self.w, self.h);
                    let src = mapped.pData as *const u8;
                    let row = self.w * 4;
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
                }
            }
            fresh
        }
    }
}

/// [`Capture`] 的 DXGI 桌面复制实现,产出 BGRA [`Frame`]。
pub struct DxgiCapture {
    inner: DxgiInner,
}

impl DxgiCapture {
    /// 尝试建立桌面复制;无 D3D / RDP / 锁屏等场景返回 `None`。
    pub fn new_primary() -> Option<Self> {
        DxgiInner::try_new().ok().map(|inner| DxgiCapture { inner })
    }
}

impl Capture for DxgiCapture {
    fn grab(&mut self) -> Result<Frame> {
        let mut frame = Frame::bgra8(0, 0, Vec::new());
        self.inner.grab_into(&mut frame);
        if frame.pixels.is_empty() {
            return Err(Error::Capture("dxgi returned empty frame".into()));
        }
        Ok(frame)
    }

    fn grab_into(&mut self, dst: &mut Frame) -> Result<bool> {
        let changed = self.inner.grab_into(dst);
        if dst.pixels.is_empty() {
            return Err(Error::Capture("dxgi returned empty frame".into()));
        }
        Ok(changed)
    }

    fn backend(&self) -> &'static str {
        "dxgi"
    }
}
