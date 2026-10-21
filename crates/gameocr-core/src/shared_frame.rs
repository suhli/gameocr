//! Bounded Windows page-file mappings. NDJSON is the ownership hand-off; the
//! parent holds an exclusive mutable Worker borrow until its response arrives.
use anyhow::{Result, ensure};
use image::RgbImage;
use serde::{Deserialize, Serialize};
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
const HEADER: usize = 64;
const MAGIC: u64 = 0x47414d454f435231;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedFrame {
    pub name: String,
    pub capacity: usize,
    pub width: u32,
    pub height: u32,
    pub frame_id: u64,
    pub context: [u64; 3],
}
impl SharedFrame {
    fn header(&self) -> Result<[u64; 8]> {
        let len = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(3))
            .ok_or_else(|| anyhow::anyhow!("Shared frame size overflow"))?;
        ensure!(
            self.width > 0
                && self.height > 0
                && self.width <= 16384
                && self.height <= 16384
                && len <= MAX_BYTES
                && self.capacity >= len + HEADER
                && self.capacity <= MAX_BYTES + HEADER,
            "Shared frame capacity invalid"
        );
        ensure!(
            self.name.starts_with("Local\\GameOCR-")
                && self.name.len() < 160
                && !self.name.contains('\0'),
            "Invalid shared frame name"
        );
        Ok([
            MAGIC,
            self.width as u64,
            self.height as u64,
            len as u64,
            self.frame_id,
            self.context[0],
            self.context[1],
            self.context[2],
        ])
    }
}
#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        ffi::c_void,
        ptr::NonNull,
        sync::atomic::{AtomicU64, Ordering},
    };
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileMappingW(
            file: *mut c_void,
            attributes: *const c_void,
            protect: u32,
            high: u32,
            low: u32,
            name: *const u16,
        ) -> *mut c_void;
        fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
        fn MapViewOfFile(
            mapping: *mut c_void,
            access: u32,
            high: u32,
            low: u32,
            bytes: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }
    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(Some(0)).collect()
    }
    pub struct Mapping {
        handle: NonNull<c_void>,
        view: NonNull<u8>,
        pub capacity: usize,
        name: String,
    }
    // Unique ownership; mutation requires &mut. No shared references to mapped bytes escape.
    unsafe impl Send for Mapping {}
    impl Drop for Mapping {
        fn drop(&mut self) {
            unsafe {
                UnmapViewOfFile(self.view.as_ptr().cast());
                CloseHandle(self.handle.as_ptr());
            }
        }
    }
    impl Mapping {
        pub fn create(bytes: usize) -> Result<Self> {
            ensure!(
                bytes > 0 && bytes <= MAX_BYTES,
                "Shared frame exceeds 64 MiB"
            );
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let name = format!(
                "Local\\GameOCR-{}-{stamp}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            );
            let capacity = bytes.next_power_of_two() + HEADER;
            let handle = unsafe {
                CreateFileMappingW(
                    -1isize as *mut c_void,
                    std::ptr::null(),
                    4,
                    0,
                    capacity as u32,
                    wide(&name).as_ptr(),
                )
            };
            let handle = NonNull::new(handle).ok_or_else(std::io::Error::last_os_error)?;
            if unsafe { GetLastError() } == 183 {
                unsafe {
                    CloseHandle(handle.as_ptr());
                }
                anyhow::bail!("Shared mapping name already exists");
            }
            Self::map(handle, capacity, name, 2)
        }
        fn map(
            handle: NonNull<c_void>,
            capacity: usize,
            name: String,
            access: u32,
        ) -> Result<Self> {
            let view = NonNull::new(
                unsafe { MapViewOfFile(handle.as_ptr(), access, 0, 0, capacity) }.cast::<u8>(),
            );
            match view {
                Some(view) => Ok(Self {
                    handle,
                    view,
                    capacity,
                    name,
                }),
                None => {
                    let e = std::io::Error::last_os_error();
                    unsafe {
                        CloseHandle(handle.as_ptr());
                    }
                    Err(e.into())
                }
            }
        }
        pub fn write(
            &mut self,
            image: &RgbImage,
            frame_id: u64,
            context: [u64; 3],
        ) -> Result<SharedFrame> {
            let descriptor = SharedFrame {
                name: self.name.clone(),
                capacity: self.capacity,
                width: image.width(),
                height: image.height(),
                frame_id,
                context,
            };
            let header = descriptor.header()?;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    image.as_raw().as_ptr(),
                    self.view.as_ptr().add(HEADER),
                    image.as_raw().len(),
                );
                std::ptr::copy_nonoverlapping(
                    header.as_ptr().cast::<u8>(),
                    self.view.as_ptr(),
                    HEADER,
                );
            }
            std::sync::atomic::fence(Ordering::Release);
            Ok(descriptor)
        }
    }
    pub fn read(descriptor: &SharedFrame) -> Result<RgbImage> {
        let expected = descriptor.header()?;
        let handle =
            NonNull::new(unsafe { OpenFileMappingW(4, 0, wide(&descriptor.name).as_ptr()) })
                .ok_or_else(std::io::Error::last_os_error)?;
        let mapping = Mapping::map(handle, descriptor.capacity, descriptor.name.clone(), 4)?;
        std::sync::atomic::fence(Ordering::Acquire);
        let actual = unsafe { std::ptr::read_unaligned(mapping.view.as_ptr().cast::<[u64; 8]>()) };
        ensure!(actual == expected, "Stale shared frame identity");
        // One owned copy: FrameCache can safely retain this frame after the parent
        // reuses its slot. This is not a zero-copy or GPU-texture transport claim.
        let data = unsafe {
            std::slice::from_raw_parts(mapping.view.as_ptr().add(HEADER), expected[3] as usize)
        }
        .to_vec();
        ensure!(
            unsafe { std::ptr::read_unaligned(mapping.view.as_ptr().cast::<[u64; 8]>()) }
                == expected,
            "Shared frame overwritten during read"
        );
        RgbImage::from_raw(descriptor.width, descriptor.height, data)
            .ok_or_else(|| anyhow::anyhow!("Invalid shared RGB buffer"))
    }
}
#[cfg(windows)]
pub use platform::{Mapping, read};
#[cfg(not(windows))]
pub fn read(_: &SharedFrame) -> Result<RgbImage> {
    anyhow::bail!("Shared frame transport requires Windows")
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn exact_bytes_identity_bounds_and_cleanup() {
        let image = RgbImage::from_pixel(64, 32, image::Rgb([1, 2, 3]));
        let mut map = Mapping::create(image.as_raw().len()).unwrap();
        let d = map.write(&image, 5, [1, 2, 3]).unwrap();
        assert_eq!(read(&d).unwrap(), image);
        let mut stale = d.clone();
        stale.frame_id = 4;
        assert!(read(&stale).is_err());
        stale = d.clone();
        stale.width = u32::MAX;
        assert!(read(&stale).is_err());
        let next = map.write(&image, 6, [2, 2, 3]).unwrap();
        assert!(read(&d).is_err());
        assert!(read(&next).is_ok());
        drop(map);
        assert!(read(&next).is_err());
        assert!(Mapping::create(MAX_BYTES + 1).is_err());
    }
}
