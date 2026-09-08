use image::RgbaImage;

pub(super) struct CapturedFrame {
    pub(super) image: RgbaImage,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(super) use windows::capture_warframe;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub(super) use x11::capture_warframe;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod unsupported;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(super) use unsupported::capture_warframe;
