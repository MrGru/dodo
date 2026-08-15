//! Rasterises an application bundle's Finder icon to the small, shareable
//! payload the results grid draws.
//!
//! This replaces a two-line `TIFFRepresentation()` that was wrong in both
//! directions, measured over the 98 bundles in `/Applications`,
//! `~/Applications`, `/System/Applications` and
//! `/System/Applications/Utilities`:
//!
//! - **It never drew.** `NSImage`'s TIFF representation of a Finder icon uses
//!   16-bit *floating point* samples, and the `image` crate — which is the
//!   decoder GPUI hands `ImageFormat::Tiff` to, via
//!   `ClipboardImage::to_image_data` — rejects those outright with
//!   "Unhandled TIFF sample format 3 for 16 bits". 97 of the 98 icons failed
//!   to decode, so `img(..).with_fallback(..)` quietly drew the category
//!   glyph for almost every row. That is the "rows show a default icon"
//!   report: the bytes were there, and unreadable.
//! - **It was enormous.** That representation carries the whole standard
//!   size ladder at 16 bits per sample: **73,949,448 bytes per application**,
//!   the same for every one of them, retained for as long as the result
//!   stayed on screen. 6.71 GiB for one Installed Apps scan.
//!
//! Drawing the icon once into a fixed [`IconRaster::EDGE_PIXELS`] square of
//! 8-bit RGBA and encoding PNG fixes both at once: **5,891 bytes per
//! application on average, 7,745 at worst, and 98 of 98 decode**. PNG rather
//! than raw RGBA because GPUI's decoder takes encoded bytes and because
//! 64×64×4 raw would be 16 KiB — larger than the encoded icon, not smaller.
//!
//! It stays on the background executor, like the scan that calls it. AppKit's
//! image drawing is not documented as main-thread-only (unlike view drawing),
//! and this touches no view hierarchy: it allocates its own bitmap, makes a
//! context over it, draws, and drops the context again, all within one call.
//!
//! Nothing here decides anything, so nothing here is unit tested — the rule
//! it must not break (the size bound) lives in `core::icon`, which is pure.
//! What no test on this machine can prove is how the icons *look* in the
//! running app; the rasters were checked by writing them out and viewing
//! them, never by running dodo.

use std::path::Path;

use objc2::AnyThread as _;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSGraphicsContext, NSImage, NSWorkspace,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

use crate::core::icon::IconRaster;

/// The bundle icon for `path`, drawn small. `None` when the path is not
/// representable to AppKit, when any step of the draw fails, or when the
/// encoded result somehow exceeds [`IconRaster::MAX_BYTES`] — every one of
/// which means the row falls back to its category glyph.
pub fn application_icon(path: &Path) -> Option<IconRaster> {
    let icon = workspace_icon(path)?;
    let bytes = rasterise_png(&icon, IconRaster::EDGE_PIXELS)?;
    IconRaster::new(bytes)
}

fn workspace_icon(path: &Path) -> Option<Retained<NSImage>> {
    let path = path.to_str()?;
    Some(NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path)))
}

/// Draws `image` once into a fresh `edge` × `edge` 8-bit RGBA bitmap and
/// returns its PNG encoding.
///
/// The `fromRect:` argument is the zero rect on purpose — AppKit reads that
/// as "the whole image", and it is what lets one call pick the representation
/// nearest the destination size and scale it, rather than this code choosing
/// a representation itself.
fn rasterise_png(image: &NSImage, edge: usize) -> Option<Vec<u8>> {
    let edge_points = edge as f64;
    // SAFETY: every call below is an ordinary AppKit drawing call on objects
    // this function itself allocates. The `null_mut()` planes argument is the
    // documented way to ask `NSBitmapImageRep` to allocate its own backing
    // store, and the graphics state is saved and restored around the draw so
    // no other drawing on this thread observes the swapped context.
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            edge as isize,
            edge as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )?;
        rep.setSize(NSSize::new(edge_points, edge_points));

        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        image.drawInRect_fromRect_operation_fraction(
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(edge_points, edge_points)),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
            NSCompositingOperation::Copy,
            1.0,
        );
        NSGraphicsContext::restoreGraphicsState_class();

        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
            .map(|data| data.to_vec())
    }
}
