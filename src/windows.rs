//! Windows backend
use owning_ref::OwningRefMut;
use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    ops::{Deref, DerefMut},
};
use winapi::{
    shared::windef::{HDC, HWND},
    um::{
        wingdi::{StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY},
        winuser::{GetDC, ReleaseDC},
    },
};
use winit::{platform::windows::WindowExtWindows, window::Window};

use super::{align::Align, buffer::Buffer, Config, Format, ImageInfo, NullContextImpl};

#[derive(Debug)]
pub struct SurfaceImpl {
    hwnd: HWND,
    image: RefCell<Buffer>,
    image_info: Cell<ImageInfo>,
    scanline_align: Align,
}

impl SurfaceImpl {
    pub(crate) unsafe fn new(window: &Window, _: &NullContextImpl, config: &Config) -> Self {
        Self {
            hwnd: window.hwnd() as _,
            image: RefCell::new(Buffer::from_size_align(1, config.align).unwrap()),
            image_info: Cell::new(ImageInfo::default()),
            scanline_align: Align::new(config.scanline_align).unwrap(),
        }
    }

    pub fn update_surface(&self, extent: [u32; 2], format: Format) {
        assert_ne!(extent[0], 0);
        assert_ne!(extent[1], 0);
        assert!(extent[0] <= <i32>::max_value() as u32);
        assert!(extent[1] <= <i32>::max_value() as u32);

        use std::convert::TryInto;
        let extent_usize: [usize; 2] = [
            extent[0].try_into().expect("overflow"),
            extent[1].try_into().expect("overflow"),
        ];

        let stride = extent_usize[0]
            .checked_mul(4)
            .and_then(|x| self.scanline_align.align_up(x))
            .expect("overflow");

        let size = stride.checked_mul(extent_usize[1]).expect("overflow");

        // `stride` is used to derive `BITMAPINFOHEADER::biWidth`, so the derived
        // value must fit in `c_int`
        let _stride_pixels: std::os::raw::c_int = (stride / 4).try_into().expect("overflow");

        let mut image = self.image.borrow_mut();
        image.resize(size);

        self.image_info.set(ImageInfo {
            extent,
            stride,
            format,
        });
    }

    pub fn supported_formats(&self) -> impl Iterator<Item = Format> + '_ {
        [Format::Argb8888].iter().cloned()
    }

    pub fn image_info(&self) -> ImageInfo {
        self.image_info.get()
    }

    pub fn num_images(&self) -> usize {
        1
    }

    pub fn does_preserve_image(&self) -> bool {
        true
    }

    pub fn poll_next_image(&self) -> Option<usize> {
        Some(0)
    }

    pub fn lock_image(&self, i: usize) -> impl Deref<Target = [u8]> + DerefMut + '_ {
        assert_eq!(i, 0);
        OwningRefMut::new(self.image.borrow_mut()).map_mut(|p| &mut **p)
    }

    pub fn present_image(&self, i: usize) {
        assert_eq!(i, 0);

        let image_info = self.image_info.get();
        let image = self
            .image
            .try_borrow()
            .expect("the image is currently locked");

        assert_eq!(image_info.format, Format::Argb8888);

        // The following value works for `Argb8888`.
        // Although the GDI's documentation says that `BI_RGB` ignores the
        // alpha channel, it still copies it to the backing store as-is, which
        // DWM interprets as the alpha channel.
        let bitmap_info_header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as _,
            biWidth: (image_info.stride / 4) as _,
            biHeight: -(image_info.extent[1] as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        let bitmap_info = &bitmap_info_header as *const BITMAPINFOHEADER as *const BITMAPINFO;

        unsafe {
            let hdc = UniqueDC::new(self.hwnd, GetDC(self.hwnd)).expect("GetDC failed");

            StretchDIBits(
                hdc.hdc(),
                0,
                0,
                image_info.extent[0] as _,
                image_info.extent[1] as _,
                0,
                0,
                image_info.extent[0] as _,
                image_info.extent[1] as _,
                image.as_ptr() as *const _,
                bitmap_info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        }
    }
}

struct UniqueDC(HWND, HDC);

impl UniqueDC {
    unsafe fn new(hwnd: HWND, hdc: HDC) -> Option<Self> {
        if hdc.is_null() {
            None
        } else {
            Some(UniqueDC(hwnd, hdc))
        }
    }

    fn hdc(&self) -> HDC {
        self.1
    }
}

impl Drop for UniqueDC {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(self.0, self.1);
        }
    }
}
