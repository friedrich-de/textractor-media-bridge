use crate::media::window::NativeHwnd;

#[derive(Debug, Clone)]
pub struct CapturedScreenshot {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub backend: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
    #[error("screenshot capture is disabled")]
    Disabled,
    #[error("window capture is unsupported on this platform")]
    #[allow(dead_code)]
    Unsupported,
    #[error("invalid window size")]
    InvalidSize,
    #[error("screenshot capture timed out")]
    Timeout,
    #[error("screenshot frame was not usable: {0}")]
    InvalidFrame(&'static str),
    #[error("screenshot capture call failed: {0}")]
    CaptureFailed(String),
    #[error("all screenshot backends failed; WGC: {wgc}; GDI: {gdi}")]
    AllBackendsFailed {
        wgc: Box<ScreenshotError>,
        gdi: Box<ScreenshotError>,
    },
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
}

#[derive(Debug, Clone)]
pub struct ScreenshotManager {
    backend: ScreenshotBackend,
}

impl ScreenshotManager {
    pub fn new(backend: impl Into<String>) -> Self {
        Self {
            backend: ScreenshotBackend::from_config(&backend.into()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.backend != ScreenshotBackend::Off
    }

    pub fn capture_window(&self, hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
        match self.backend {
            ScreenshotBackend::Off => Err(ScreenshotError::Disabled),
            ScreenshotBackend::Gdi => capture_gdi(hwnd),
            ScreenshotBackend::Wgc => capture_wgc(hwnd),
            ScreenshotBackend::Auto => capture_auto(hwnd),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenshotBackend {
    Off,
    Auto,
    Wgc,
    Gdi,
}

impl ScreenshotBackend {
    fn from_config(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "off" => Self::Off,
            "gdi" | "win32-gdi" => Self::Gdi,
            "wgc" | "windows-graphics-capture" => Self::Wgc,
            "auto" | "" => Self::Auto,
            _ => Self::Auto,
        }
    }
}

fn capture_auto(hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
    match capture_wgc(hwnd) {
        Ok(wgc) if frame_is_plausible(&wgc.bytes) => Ok(wgc),
        Ok(wgc) => match capture_gdi(hwnd) {
            Ok(gdi) if frame_is_plausible(&gdi.bytes) => Ok(gdi),
            Ok(_) | Err(_) => Ok(wgc),
        },
        Err(wgc_error) => {
            capture_gdi(hwnd).map_err(|gdi_error| ScreenshotError::AllBackendsFailed {
                wgc: Box::new(wgc_error),
                gdi: Box::new(gdi_error),
            })
        }
    }
}

fn frame_is_plausible(png_bytes: &[u8]) -> bool {
    let Ok(image) = image::load_from_memory(png_bytes) else {
        return false;
    };
    let rgba = image.to_rgba8();
    let mut pixels = rgba.pixels();
    let Some(first) = pixels.next() else {
        return false;
    };

    let first = first.0;
    let mut sampled = 1usize;
    let mut different = 0usize;
    let stride = (rgba.width() as usize * rgba.height() as usize / 4096).max(1);

    for pixel in rgba.pixels().step_by(stride) {
        sampled += 1;
        let pixel = pixel.0;
        let delta = pixel
            .iter()
            .zip(first.iter())
            .map(|(left, right)| left.abs_diff(*right) as u16)
            .sum::<u16>();
        if delta > 12 {
            different += 1;
        }
        if different >= 8 {
            return true;
        }
    }

    sampled < 32 || different >= 2
}

fn capture_wgc(hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
    platform_wgc_capture_window(hwnd)
}

fn capture_gdi(hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
    platform_capture_window(hwnd)
}

#[cfg(windows)]
fn platform_capture_window(hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
    use image::{DynamicImage, ImageFormat, RgbaImage};
    use std::{io::Cursor, mem};
    use windows_sys::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
            GetWindowDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
            DIB_RGB_COLORS, HGDIOBJ, RGBQUAD, SRCCOPY,
        },
        UI::WindowsAndMessaging::GetWindowRect,
    };

    unsafe {
        let hwnd = hwnd as HWND;
        let mut rect = mem::zeroed::<RECT>();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Err(ScreenshotError::CaptureFailed("GetWindowRect".to_owned()));
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return Err(ScreenshotError::InvalidSize);
        }

        let hdc_window = GetWindowDC(hwnd);
        if hdc_window.is_null() {
            return Err(ScreenshotError::CaptureFailed("GetWindowDC".to_owned()));
        }

        let hdc_mem = CreateCompatibleDC(hdc_window);
        if hdc_mem.is_null() {
            let _ = ReleaseDC(hwnd, hdc_window);
            return Err(ScreenshotError::CaptureFailed(
                "CreateCompatibleDC".to_owned(),
            ));
        }

        let bitmap = CreateCompatibleBitmap(hdc_window, width, height);
        if bitmap.is_null() {
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(hwnd, hdc_window);
            return Err(ScreenshotError::CaptureFailed(
                "CreateCompatibleBitmap".to_owned(),
            ));
        }

        let old = SelectObject(hdc_mem, bitmap as HGDIOBJ);
        let blit_ok = BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            hdc_window,
            0,
            0,
            SRCCOPY | CAPTUREBLT,
        ) != 0;
        if !blit_ok {
            if !old.is_null() {
                let _ = SelectObject(hdc_mem, old);
            }
            let _ = DeleteObject(bitmap as HGDIOBJ);
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(hwnd, hdc_window);
            return Err(ScreenshotError::CaptureFailed("BitBlt".to_owned()));
        }

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };

        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let got = GetDIBits(
            hdc_mem,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );

        if !old.is_null() {
            let _ = SelectObject(hdc_mem, old);
        }
        let _ = DeleteObject(bitmap as HGDIOBJ);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(hwnd, hdc_window);

        if got == 0 {
            return Err(ScreenshotError::CaptureFailed("GetDIBits".to_owned()));
        }

        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
            chunk[3] = 255;
        }

        let Some(image) = RgbaImage::from_raw(width as u32, height as u32, pixels) else {
            return Err(ScreenshotError::InvalidSize);
        };
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut cursor, ImageFormat::Png)?;

        Ok(CapturedScreenshot {
            bytes: cursor.into_inner(),
            width: width as u32,
            height: height as u32,
            backend: "win32-gdi",
        })
    }
}

#[cfg(windows)]
fn platform_wgc_capture_window(hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
    windows_wgc::capture_window(hwnd)
}

#[cfg(not(windows))]
fn platform_wgc_capture_window(_hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
    Err(ScreenshotError::Unsupported)
}

#[cfg(not(windows))]
fn platform_capture_window(_hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
    Err(ScreenshotError::Unsupported)
}

#[cfg(windows)]
mod windows_wgc {
    use super::{CapturedScreenshot, NativeHwnd, ScreenshotError};
    use image::{DynamicImage, ImageFormat, RgbaImage};
    use std::{
        io::Cursor,
        sync::mpsc,
        time::{Duration, Instant},
    };
    use windows::{
        core::{Interface, HSTRING},
        Foundation::TypedEventHandler,
        Graphics::{
            Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
            DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        },
        Win32::{
            Foundation::{HMODULE, HWND},
            Graphics::{
                Direct3D::{
                    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
                    D3D_FEATURE_LEVEL_11_1,
                },
                Direct3D11::{
                    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
                    ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
                    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
                },
                Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                Dxgi::{Common::DXGI_SAMPLE_DESC, IDXGIDevice},
            },
            System::WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
                RoGetActivationFactory, RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED,
            },
        },
    };

    const FRAME_TIMEOUT: Duration = Duration::from_millis(900);

    pub fn capture_window(hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
        let _apartment = WinRtApartment::initialize()?;
        if !GraphicsCaptureSession::IsSupported()
            .map_err(|error| ScreenshotError::CaptureFailed(format!("WGC IsSupported: {error}")))?
        {
            return Err(ScreenshotError::Unsupported);
        }

        capture_window_inner(hwnd)
    }

    fn capture_window_inner(hwnd: NativeHwnd) -> Result<CapturedScreenshot, ScreenshotError> {
        let item = create_capture_item(hwnd)?;
        let size = item
            .Size()
            .map_err(|error| ScreenshotError::CaptureFailed(format!("WGC item size: {error}")))?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(ScreenshotError::InvalidSize);
        }

        let d3d = create_d3d_device()?;
        let winrt_device = create_winrt_device(&d3d.device)?;
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )
        .map_err(|error| ScreenshotError::CaptureFailed(format!("WGC frame pool: {error}")))?;

        let session = frame_pool
            .CreateCaptureSession(&item)
            .map_err(|error| ScreenshotError::CaptureFailed(format!("WGC session: {error}")))?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);

        let (sender, receiver) = mpsc::channel();
        let token = frame_pool
            .FrameArrived(&TypedEventHandler::<
                Direct3D11CaptureFramePool,
                windows::core::IInspectable,
            >::new(move |pool, _| {
                let _ = sender.send(pool.ok().and_then(|pool| pool.TryGetNextFrame()));
                Ok(())
            }))
            .map_err(|error| ScreenshotError::CaptureFailed(format!("WGC event: {error}")))?;

        session.StartCapture().map_err(|error| {
            ScreenshotError::CaptureFailed(format!("WGC StartCapture: {error}"))
        })?;

        let frame = receive_frame(&receiver)?;
        let pixels = read_frame_pixels(&d3d, &frame)?;
        let _ = frame.Close();
        let _ = frame_pool.RemoveFrameArrived(token);
        let _ = session.Close();
        let _ = frame_pool.Close();

        Ok(CapturedScreenshot {
            bytes: encode_png(pixels.width, pixels.height, pixels.rgba)?,
            width: pixels.width,
            height: pixels.height,
            backend: "windows-graphics-capture",
        })
    }

    fn receive_frame(
        receiver: &mpsc::Receiver<
            windows::core::Result<windows::Graphics::Capture::Direct3D11CaptureFrame>,
        >,
    ) -> Result<windows::Graphics::Capture::Direct3D11CaptureFrame, ScreenshotError> {
        let deadline = Instant::now() + FRAME_TIMEOUT;
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(ScreenshotError::Timeout);
            };
            match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
                Ok(Ok(frame)) => return Ok(frame),
                Ok(Err(error)) => {
                    return Err(ScreenshotError::CaptureFailed(format!(
                        "WGC next frame: {error}"
                    )));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(ScreenshotError::CaptureFailed(
                        "WGC frame event disconnected".to_owned(),
                    ));
                }
            }
        }
    }

    fn create_capture_item(hwnd: NativeHwnd) -> Result<GraphicsCaptureItem, ScreenshotError> {
        let factory: IGraphicsCaptureItemInterop = unsafe {
            RoGetActivationFactory(&HSTRING::from(
                "Windows.Graphics.Capture.GraphicsCaptureItem",
            ))
        }
        .map_err(|error| {
            ScreenshotError::CaptureFailed(format!("WGC activation factory: {error}"))
        })?;
        unsafe { factory.CreateForWindow(HWND(hwnd as _)) }.map_err(|error| {
            ScreenshotError::CaptureFailed(format!("WGC CreateForWindow: {error}"))
        })
    }

    struct D3DContext {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
    }

    fn create_d3d_device() -> Result<D3DContext, ScreenshotError> {
        let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        let mut device = None;
        let mut context = None;
        let mut selected_level = D3D_FEATURE_LEVEL::default();

        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut selected_level),
                Some(&mut context),
            )
        }
        .map_err(|error| ScreenshotError::CaptureFailed(format!("D3D11CreateDevice: {error}")))?;

        let device = device.ok_or_else(|| {
            ScreenshotError::CaptureFailed("D3D11CreateDevice returned no device".to_owned())
        })?;
        let context = context.ok_or_else(|| {
            ScreenshotError::CaptureFailed("D3D11CreateDevice returned no context".to_owned())
        })?;

        Ok(D3DContext { device, context })
    }

    fn create_winrt_device(device: &ID3D11Device) -> Result<IDirect3DDevice, ScreenshotError> {
        let dxgi_device: IDXGIDevice = device.cast().map_err(|error| {
            ScreenshotError::CaptureFailed(format!("IDXGIDevice cast: {error}"))
        })?;
        let inspectable =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }.map_err(|error| {
                ScreenshotError::CaptureFailed(format!(
                    "CreateDirect3D11DeviceFromDXGIDevice: {error}"
                ))
            })?;
        inspectable.cast().map_err(|error| {
            ScreenshotError::CaptureFailed(format!("IDirect3DDevice cast: {error}"))
        })
    }

    struct FramePixels {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    }

    fn read_frame_pixels(
        d3d: &D3DContext,
        frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
    ) -> Result<FramePixels, ScreenshotError> {
        let content_size = frame.ContentSize().map_err(|error| {
            ScreenshotError::CaptureFailed(format!("WGC content size: {error}"))
        })?;
        let width = u32::try_from(content_size.Width).map_err(|_| ScreenshotError::InvalidSize)?;
        let height =
            u32::try_from(content_size.Height).map_err(|_| ScreenshotError::InvalidSize)?;
        if width == 0 || height == 0 {
            return Err(ScreenshotError::InvalidSize);
        }

        let surface = frame
            .Surface()
            .map_err(|error| ScreenshotError::CaptureFailed(format!("WGC surface: {error}")))?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|error| {
            ScreenshotError::CaptureFailed(format!("IDirect3DDxgiInterfaceAccess cast: {error}"))
        })?;
        let source: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(|error| {
            ScreenshotError::CaptureFailed(format!("WGC texture access: {error}"))
        })?;

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        let mut staging = None;
        unsafe { d3d.device.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(
            |error| ScreenshotError::CaptureFailed(format!("D3D CreateTexture2D staging: {error}")),
        )?;
        let staging = staging.ok_or_else(|| {
            ScreenshotError::CaptureFailed(
                "D3D CreateTexture2D returned no staging texture".to_owned(),
            )
        })?;

        let source_resource: ID3D11Resource = source.cast().map_err(|error| {
            ScreenshotError::CaptureFailed(format!("source texture resource cast: {error}"))
        })?;
        let staging_resource: ID3D11Resource = staging.cast().map_err(|error| {
            ScreenshotError::CaptureFailed(format!("staging texture resource cast: {error}"))
        })?;
        unsafe {
            d3d.context
                .CopyResource(&staging_resource, &source_resource);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            d3d.context
                .Map(&staging_resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|error| ScreenshotError::CaptureFailed(format!("D3D Map staging: {error}")))?;

        let rgba = copy_bgra_to_rgba(
            mapped.pData.cast::<u8>(),
            mapped.RowPitch as usize,
            width,
            height,
        );
        unsafe {
            d3d.context.Unmap(&staging_resource, 0);
        }

        Ok(FramePixels {
            width,
            height,
            rgba: rgba?,
        })
    }

    fn copy_bgra_to_rgba(
        source: *const u8,
        row_pitch: usize,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, ScreenshotError> {
        if source.is_null() {
            return Err(ScreenshotError::InvalidFrame(
                "mapped texture pointer was null",
            ));
        }
        let width = width as usize;
        let height = height as usize;
        let row_bytes = width.checked_mul(4).ok_or(ScreenshotError::InvalidSize)?;
        if row_pitch < row_bytes {
            return Err(ScreenshotError::InvalidFrame(
                "mapped texture row pitch was too small",
            ));
        }

        let mut rgba = vec![0u8; row_bytes * height];
        for y in 0..height {
            let src_row =
                unsafe { std::slice::from_raw_parts(source.add(y * row_pitch), row_bytes) };
            let dst_row = &mut rgba[y * row_bytes..(y + 1) * row_bytes];
            for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = 255;
            }
        }
        Ok(rgba)
    }

    fn encode_png(width: u32, height: u32, rgba: Vec<u8>) -> Result<Vec<u8>, ScreenshotError> {
        let image = RgbaImage::from_raw(width, height, rgba).ok_or(ScreenshotError::InvalidSize)?;
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut cursor, ImageFormat::Png)?;
        Ok(cursor.into_inner())
    }

    struct WinRtApartment;

    impl WinRtApartment {
        fn initialize() -> Result<Self, ScreenshotError> {
            unsafe {
                RoInitialize(RO_INIT_MULTITHREADED).map_err(|error| {
                    ScreenshotError::CaptureFailed(format!("RoInitialize: {error}"))
                })?;
            }
            Ok(Self)
        }
    }

    impl Drop for WinRtApartment {
        fn drop(&mut self) {
            unsafe {
                RoUninitialize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::frame_is_plausible;

    #[test]
    fn frame_validation_accepts_non_flat_images() {
        let mut image = image::RgbaImage::new(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                image.put_pixel(x, y, image::Rgba([x as u8, y as u8, 128, 255]));
            }
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();

        assert!(frame_is_plausible(&bytes.into_inner()));
    }

    #[test]
    fn frame_validation_rejects_large_flat_images() {
        let image = image::RgbaImage::from_pixel(128, 128, image::Rgba([0, 0, 0, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();

        assert!(!frame_is_plausible(&bytes.into_inner()));
    }
}
