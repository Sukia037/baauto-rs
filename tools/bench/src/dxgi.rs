//! DXGI owns all unsafe capture operations. Never linked into the engine's capture path.
use crate::Pixels;
use baauto_config::Ticks;
use baauto_win_platform::geometry::Rect;
use std::{marker::PhantomData, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
    },
    core::{Error, Interface, Result},
};
fn fail(message: &str) -> Error {
    Error::new(E_FAIL, message)
}
pub struct Capture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    size: [u32; 2],
    _thread: PhantomData<Rc<()>>,
}
struct FrameGuard<'a>(&'a IDXGIOutputDuplication);
impl Drop for FrameGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: Created only after a successful AcquireNextFrame; all borrowed resources are finished before the guard is dropped.
        unsafe {
            let _ = self.0.ReleaseFrame();
        }
    }
}
struct MapGuard<'a>(&'a ID3D11DeviceContext, &'a ID3D11Texture2D);
impl Drop for MapGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: Created only after a successful Map on subresource 0; exactly one matching Unmap occurs before the texture is reused.
        unsafe {
            self.0.Unmap(self.1, 0);
        }
    }
}
impl Capture {
    pub fn new(monitor: Rect) -> Result<Self> {
        // SAFETY: COM smart pointers own returned interfaces. Output/device structs stay valid for every synchronous call; no raw pointer escapes.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            let mut selected = None;
            for a in 0..64 {
                let adapter = match factory.EnumAdapters1(a) {
                    Ok(a) => a,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(e),
                };
                for index in 0..64 {
                    let output = match adapter.EnumOutputs(index) {
                        Ok(o) => o,
                        Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                        Err(e) => return Err(e),
                    };
                    let desc = output.GetDesc()?;
                    let r = desc.DesktopCoordinates;
                    if r.left == monitor.left
                        && r.top == monitor.top
                        && r.right - r.left == monitor.width
                        && r.bottom - r.top == monitor.height
                    {
                        if desc.Rotation != DXGI_MODE_ROTATION_IDENTITY {
                            return Err(fail(
                                "rotated desktop is unsupported; fullscreen 16:9 required",
                            ));
                        }
                        selected = Some((adapter, output));
                        break;
                    }
                }
                if selected.is_some() {
                    break;
                }
            }
            let (adapter, output) =
                selected.ok_or_else(|| fail("game monitor not found in DXGI outputs"))?;
            let adapter: IDXGIAdapter = adapter.cast()?;
            let (mut device, mut context) = (None, None);
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
            let device = device.ok_or_else(|| fail("D3D11 device absent"))?;
            let context = context.ok_or_else(|| fail("D3D11 context absent"))?;
            let output: IDXGIOutput1 = output.cast()?;
            let duplication = output.DuplicateOutput(&device)?;
            Ok(Self {
                device,
                context,
                duplication,
                staging: None,
                size: [0; 2],
                _thread: PhantomData,
            })
        }
    }
    /// Waits at most timeout_ms on a bench worker; pointer-only updates are ignored.
    pub fn next_frame(&mut self, timeout_ms: u32) -> Result<Option<Pixels>> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // SAFETY: Stack output structures remain live for the synchronous acquisition call; this Capture is confined to its creating worker.
        match unsafe {
            self.duplication
                .AcquireNextFrame(timeout_ms, &mut info, &mut resource)
        } {
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        let _frame = FrameGuard(&self.duplication);
        if info.LastPresentTime <= 0 {
            return Ok(None);
        }
        if info.ProtectedContentMaskedOut.as_bool() {
            return Err(fail(
                "protected content masked; detector cannot validate this frame",
            ));
        }
        let texture: ID3D11Texture2D = resource
            .ok_or_else(|| fail("acquired desktop resource absent"))?
            .cast()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: Live texture interface writes a complete descriptor into the stack allocation.
        unsafe {
            texture.GetDesc(&mut desc);
        }
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(fail("unsupported desktop pixel format"));
        }
        let row_bytes = (desc.Width as usize)
            .checked_mul(4)
            .ok_or_else(|| fail("frame width overflow"))?;
        let length = row_bytes
            .checked_mul(desc.Height as usize)
            .ok_or_else(|| fail("frame size overflow"))?;
        if length == 0 || length > 512 * 1024 * 1024 {
            return Err(fail("invalid desktop frame dimensions"));
        }
        if self.staging.is_none() || self.size != [desc.Width, desc.Height] {
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..desc
            };
            let mut staging = None;
            // SAFETY: Descriptor retains the source's dimensions/format; no initial data; output smart pointer is owned on success.
            unsafe {
                self.device
                    .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            }
            self.staging = staging;
            self.size = [desc.Width, desc.Height];
        }
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| fail("staging texture absent"))?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: Source and staging are compatible owned textures, subresource 0 exists, and Map output is live through the call.
        unsafe {
            self.context.CopyResource(staging, &texture);
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        }
        let _map = MapGuard(&self.context, staging);
        if mapped.pData.is_null() || (mapped.RowPitch as usize) < row_bytes {
            return Err(fail("invalid mapped row pitch"));
        }
        let mut bgra = Vec::with_capacity(length);
        for row in 0..desc.Height as usize {
            let offset = row
                .checked_mul(mapped.RowPitch as usize)
                .ok_or_else(|| fail("row offset overflow"))?;
            // SAFETY: Map guarantees at least RowPitch bytes per row for desc.Height rows. Bounds above ensure each copied row fits; Unmap occurs after all copies.
            unsafe {
                bgra.extend_from_slice(std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(offset),
                    row_bytes,
                ));
            }
        }
        Ok(Some(Pixels {
            width: desc.Width,
            height: desc.Height,
            bgra,
            present: Ticks(info.LastPresentTime as u64),
        }))
    }
}
