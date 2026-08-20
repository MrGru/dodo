//! **§10's pictures, as GPUI elements** — the decode cache, the crop as a
//! clip, and the one thing the canvas paints that is not a `Window::paint_*`
//! call.
//!
//! # Why an element rather than `Window::paint_image`
//!
//! GPUI will paint a bitmap into a rectangle in one call, and it was the
//! obvious way to do this. It cannot carry an **opacity**: `paint_image` reads
//! the sprite's alpha from `Window::element_opacity`, which is `pub(crate)` and
//! is written by exactly one thing in the whole framework — a styled element's
//! own paint. The property panel gives every kind an Opacity row, images
//! included, so painting them with the raw call would have shipped a control
//! that writes a field no painter reads. That is the failure this crate has now
//! met three times under three different names, and
//! [`PictureElement`](crate::render::painter::PictureElement) is the shape of
//! not meeting it a fourth.
//!
//! An element buys three things the call does not, and the crop needs two of
//! them:
//!
//! ```text
//! div  .opacity(a) .overflow_hidden()      <- the opacity, and the crop's clip
//!  └─ img(RenderImage) .absolute() .w(sw) .h(sh) .left(-ox) .top(-oy)
//!                                          <- the crop, as a scaled offset
//! ```
//!
//! **The crop is arithmetic on the child's box.** To show the sub-rectangle
//! `crop` of a picture inside a frame of `w × h`, the whole picture is laid out
//! at `w / crop.width` by `h / crop.height` and slid up and left by the crop's
//! own origin at that scale; the parent's clip does the rest. No pixel is read,
//! no buffer is copied, and two elements cropping the same resource differently
//! share one decoded image — which is §10's rule holding one layer further down
//! than the document.
//!
//! # The decode cache, and what bounds it
//!
//! A decoded picture is BGRA at full resolution — a 4,000 × 3,000 photograph is
//! **48 MB** where its PNG was four. So [`ImageCache`] is byte-bounded like
//! §23's geometry cache, with the same eviction discipline
//! ([`EVICT_TO_FRACTION`](crate::render::cache::EVICT_TO_FRACTION)): evicting to
//! exactly the bound re-sorts the whole entry set on every insert past it, which
//! Phase 5 measured as a 22-second test and a per-frame pathology.
//!
//! Its keys are [`ImageHandle`]s, so the *cache* shares an entry between two
//! elements showing one picture for the same reason the document shares the
//! bytes: there is nowhere to put a second copy.
//!
//! **A decode is synchronous and happens on the frame that first needs the
//! picture.** That is a real hitch — tens of milliseconds for a large JPEG — and
//! it is the honest trade for this phase: the alternative is a placeholder that
//! becomes a picture a frame or two later, which needs a spawn, a notify and a
//! second state for "decoding", and the insert path already decodes off the UI
//! thread before the element exists. What is left paying it is a *loaded*
//! document's first frame, once per picture.

use std::{collections::HashMap, sync::Arc};

use gpui::{
    AnyElement, App, AvailableSpace, ImageFormat as GpuiImageFormat, IntoElement, ObjectFit,
    ParentElement, RenderImage, Styled, StyledImage, Window, div, img, point, px, size,
};
use gpui_component::ActiveTheme;

use crate::{
    geometry::Vec2,
    models::{ImageCrop, ImageFormat, ImageHandle, ImageResource},
    render::painter::PictureElement,
};

/// How many bytes of decoded picture the cache may hold.
///
/// The same order as §23's geometry bound and for the same reason — it is a
/// per-document working set rather than a library — and generous, because the
/// alternative to holding a decoded photograph is decoding it again on the next
/// frame that shows it.
pub const IMAGE_CACHE_MAX_BYTES: usize = 192 * 1024 * 1024;

/// **Decoded pictures, keyed by the handle that names their bytes.**
///
/// One entry per distinct picture, never per element. See the module doc for
/// the bound and for why a decode happens where it does.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<ImageHandle, Entry>,
    bytes: usize,
    /// A monotonic tick, bumped on every *use*. The eviction order — see
    /// [`ImageCache::evict`] — and cheaper than a timestamp.
    clock: u64,
}

struct Entry {
    image: Option<Arc<RenderImage>>,
    /// What holding this costs, in decoded bytes. Zero for a picture that
    /// failed to decode, which is still an entry: **a broken file must be
    /// refused once and not on every frame**.
    bytes: usize,
    used: u64,
}

impl ImageCache {
    pub fn new() -> ImageCache {
        ImageCache::default()
    }

    /// How many pictures are held. **The sharing rule as a number**, and what
    /// the tests assert against.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// The decoded picture for `resource`, decoding it if this is the first
    /// time anybody asked.
    ///
    /// `None` for bytes no decoder would take — a file renamed to `.png`, a
    /// truncated download — and the `None` is **remembered**, so a broken
    /// picture costs one failed decode rather than one per frame.
    pub fn decoded(
        &mut self,
        handle: ImageHandle,
        resource: &ImageResource,
        cx: &App,
    ) -> Option<Arc<RenderImage>> {
        self.clock += 1;
        let clock = self.clock;

        if let Some(entry) = self.entries.get_mut(&handle) {
            entry.used = clock;
            return entry.image.clone();
        }

        let image = decode(resource, cx);
        let bytes = image
            .as_ref()
            .map(|image| decoded_bytes(image))
            .unwrap_or(0);
        self.entries.insert(
            handle,
            Entry {
                image: image.clone(),
                bytes,
                used: clock,
            },
        );
        self.bytes += bytes;
        self.evict();
        image
    }

    /// **Files an already-decoded picture**, for the insert path.
    ///
    /// The insert decodes before the resource exists — it is where the pixel
    /// dimensions come from — and this is what stops that work being thrown
    /// away and repeated on the very next frame.
    pub fn prime(&mut self, handle: ImageHandle, image: Arc<RenderImage>) {
        self.clock += 1;
        let bytes = decoded_bytes(&image);
        if let Some(previous) = self.entries.insert(
            handle,
            Entry {
                image: Some(image),
                bytes,
                used: self.clock,
            },
        ) {
            self.bytes = self.bytes.saturating_sub(previous.bytes);
        }
        self.bytes += bytes;
        self.evict();
    }

    /// Drops least-recently-used entries until the cache is back under
    /// [`EVICT_TO_FRACTION`](crate::render::cache::EVICT_TO_FRACTION) of its
    /// bound.
    ///
    /// **Under, not to** — see that constant. Evicting to exactly the bound
    /// makes every insert past it a full re-sort, which is the pathology
    /// Phase 5 recorded in `render::cache`.
    fn evict(&mut self) {
        if self.bytes <= IMAGE_CACHE_MAX_BYTES {
            return;
        }

        let target =
            (IMAGE_CACHE_MAX_BYTES as f32 * crate::render::cache::EVICT_TO_FRACTION) as usize;
        let mut order: Vec<(u64, ImageHandle)> = self
            .entries
            .iter()
            .map(|(handle, entry)| (entry.used, *handle))
            .collect();
        order.sort_unstable();

        for (_, handle) in order {
            if self.bytes <= target {
                break;
            }
            if let Some(entry) = self.entries.remove(&handle) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
    }
}

/// What one decoded frame occupies: four bytes a pixel, times the frames an
/// animation carries.
fn decoded_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            (size.width.0.max(0) as usize) * (size.height.0.max(0) as usize) * 4
        })
        .sum()
}

/// One resource, through GPUI's own decoder.
///
/// **No new package.** `gpui::Image::to_image_data` is the same path
/// `img("file.png")` takes, `gpui` already depends on `image` 0.25 with every
/// format below among its features, and dodo is deliberate about its graph —
/// see `deny.toml`. What this crate adds is the mapping from a document's
/// format tag to GPUI's, which is one `match` and is here rather than in
/// `models/` because `models/` may not name a UI framework.
fn decode(resource: &ImageResource, cx: &App) -> Option<Arc<RenderImage>> {
    decode_bytes(resource.format, &resource.bytes, cx)
}

/// The same decode, before there is a resource to hold the bytes.
///
/// The insert path needs it in that order: a resource carries its pixel
/// dimensions, and the only thing that knows them is a decoder. So the file is
/// decoded once, its size is read off the result, and the resource is built
/// from both — with the decoded image [`primed`](ImageCache::prime) into the
/// cache so the frame that follows does not decode it a second time.
pub fn decode_bytes(format: ImageFormat, bytes: &[u8], cx: &App) -> Option<Arc<RenderImage>> {
    let format = match format {
        ImageFormat::Png => GpuiImageFormat::Png,
        ImageFormat::Jpeg => GpuiImageFormat::Jpeg,
        ImageFormat::Gif => GpuiImageFormat::Gif,
        ImageFormat::Webp => GpuiImageFormat::Webp,
        ImageFormat::Bmp => GpuiImageFormat::Bmp,
        ImageFormat::Ico => GpuiImageFormat::Ico,
        ImageFormat::Tiff => GpuiImageFormat::Tiff,
    };

    gpui::Image::from_bytes(format, bytes.to_vec())
        .to_image_data(cx.svg_renderer())
        .ok()
}

/// The pixel size of a decoded picture, as two plain numbers.
pub fn decoded_size(image: &RenderImage) -> (u32, u32) {
    let size = image.size(0);
    (size.width.0.max(0) as u32, size.height.0.max(0) as u32)
}

/// **The child's box, for a crop** — the whole of "a crop is metadata".
///
/// Returns the size the *whole* picture is laid out at and the offset it is slid
/// by, both in screen pixels, so that `crop` exactly fills `frame`. Pure, so the
/// arithmetic that decides what a user sees is a test rather than something to
/// squint at.
///
/// An uncropped picture answers the frame itself and no offset, which is what
/// makes the common case free of everything below.
pub fn crop_layout(frame: Vec2, crop: ImageCrop) -> (Vec2, Vec2) {
    let crop = crop.clamped();
    let size = Vec2::new(frame.x / crop.width, frame.y / crop.height);
    let offset = Vec2::new(size.x * crop.x, size.y * crop.y);
    (size, offset)
}

/// **Builds and lays out one picture per visible image element.**
///
/// Called from the canvas's *prepaint* — the only phase GPUI allows an element
/// to be laid out in — and handed to the painter for the paint phase, where
/// [`PaintPlan::paint_into`] emits them in the contract's order. See
/// [`PictureElement`].
///
/// **From the snapshot rather than from the plan**, and that is forced rather
/// than chosen: the plan is built during *paint*, one phase after this runs. The
/// snapshot is `render`'s, a phase earlier again, and it is where every
/// element's screen rectangle already comes from — so the two agree by
/// construction. The plan is the smaller of the two (it applies the frame's
/// clip), so this can lay out a picture the frame then does not paint; that
/// costs one layout of an off-screen box and cannot lose one, which is the
/// direction to err in.
pub fn prepaint(
    snapshot: &crate::render::RenderSnapshot,
    world: &crate::runtime::GraphWorld,
    cache: &mut ImageCache,
    origin: Vec2,
    window: &mut Window,
    cx: &mut App,
) -> Vec<PictureElement> {
    let mut pictures = Vec::new();

    for canvas in snapshot.canvas() {
        if canvas.body != crate::runtime::NodeShape::Image {
            continue;
        }
        let Some(image) = world.nodes().cold(canvas.node).image else {
            continue;
        };

        let decoded = world
            .image(image.handle)
            .and_then(|resource| cache.decoded(image.handle, resource, cx));

        let frame = canvas.screen.normalized();
        let style = world.nodes().style(canvas.node);
        let mut element = picture(
            frame.size,
            image.crop,
            style.opacity,
            world_to_screen_radius(style.corner_radius, snapshot),
            decoded,
            cx,
        );

        let placed = frame.origin + origin;
        element.prepaint_as_root(
            point(px(placed.x), px(placed.y)),
            size(
                AvailableSpace::Definite(px(frame.size.x)),
                AvailableSpace::Definite(px(frame.size.y)),
            ),
            window,
            cx,
        );

        pictures.push(PictureElement {
            node: canvas.node,
            element,
        });
    }

    pictures
}

/// A world-space corner radius in screen pixels, at the camera the snapshot was
/// extracted under.
///
/// Read from the snapshot's own anchor rather than from the live viewport,
/// because the two can differ by one frame — the snapshot is `render`'s and the
/// viewport can have moved since — and a picture laid out at one zoom and
/// painted at another would round its corners by the wrong amount.
fn world_to_screen_radius(radius: f32, snapshot: &crate::render::RenderSnapshot) -> f32 {
    let zoom = snapshot.anchor().map(|anchor| anchor.zoom).unwrap_or(1.0);
    radius * if zoom > 0.0 { zoom } else { 1.0 }
}

/// One picture, or the placeholder that stands in for one nobody could decode.
///
/// **The corner radius is put on the `img` rather than on the frame**, and only
/// when the picture is uncropped. GPUI's content mask is a rectangle with no
/// radii, so a rounded parent does not round what it clips; the sprite's own
/// `corner_radii` does, and it rounds the corners of *the whole picture* — which
/// are off-screen once a crop is in play. So a cropped picture has square
/// corners, and that is recorded rather than faked.
fn picture(
    frame: Vec2,
    crop: ImageCrop,
    opacity: f32,
    corner_radius: f32,
    decoded: Option<Arc<RenderImage>>,
    cx: &App,
) -> AnyElement {
    let radius = px(corner_radius.max(0.0));
    let opacity = opacity.clamp(0.0, 1.0);

    let Some(decoded) = decoded else {
        // **A picture nobody could decode is an empty frame, not nothing.** The
        // element is still there, still selectable and still deletable; drawing
        // nothing would leave something that can be clicked and cannot be seen.
        return div()
            .w(px(frame.x.abs()))
            .h(px(frame.y.abs()))
            .rounded(radius)
            .bg(cx.theme().muted)
            .border_1()
            .border_color(cx.theme().border)
            .opacity(opacity)
            .into_any_element();
    };

    let (size, offset) = crop_layout(Vec2::new(frame.x.abs(), frame.y.abs()), crop);
    let uncropped = crop.is_full();

    let mut picture = img(decoded)
        .absolute()
        .left(px(-offset.x))
        .top(px(-offset.y))
        .w(px(size.x))
        .h(px(size.y))
        // `Fill` rather than the default `Contain`: the box above is exactly
        // the box the crop arithmetic computed, and letterboxing inside it
        // would show a slice of the picture that is not the one asked for.
        .object_fit(ObjectFit::Fill);
    if uncropped {
        picture = picture.rounded(radius);
    }

    div()
        .relative()
        .w(px(frame.x.abs()))
        .h(px(frame.y.abs()))
        // The clip that makes a crop a crop.
        .overflow_hidden()
        .opacity(opacity)
        .child(picture)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::crop_layout;
    use crate::{geometry::Vec2, models::ImageCrop};

    /// **An uncropped picture is laid out at exactly its frame**, which is what
    /// keeps the common case free of the arithmetic above.
    #[test]
    fn no_crop_is_no_offset() {
        let (size, offset) = crop_layout(Vec2::new(200.0, 100.0), ImageCrop::FULL);

        assert_eq!(size, Vec2::new(200.0, 100.0));
        assert_eq!(offset, Vec2::ZERO);
    }

    /// **The crop window lands exactly on the frame.** Stated as the inverse of
    /// the layout: the sub-rectangle of the laid-out picture that the parent's
    /// clip keeps has to be the frame, or the user is looking at the wrong part
    /// of their photograph.
    #[test]
    fn the_cropped_window_fills_the_frame() {
        let frame = Vec2::new(300.0, 150.0);
        let crop = ImageCrop::new(0.25, 0.5, 0.5, 0.25);

        let (size, offset) = crop_layout(frame, crop);

        // The whole picture is four times as wide and four times as tall as the
        // window it is being seen through.
        assert!((size.x - 600.0).abs() < 1e-3, "{size:?}");
        assert!((size.y - 600.0).abs() < 1e-3, "{size:?}");
        // And it is slid so that the crop's own origin sits at the frame's.
        assert!((offset.x - 150.0).abs() < 1e-3, "{offset:?}");
        assert!((offset.y - 300.0).abs() < 1e-3, "{offset:?}");

        // The window kept by the clip: from the offset, one frame wide.
        let shown_origin = Vec2::new(offset.x / size.x, offset.y / size.y);
        let shown_size = Vec2::new(frame.x / size.x, frame.y / size.y);
        assert!((shown_origin.x - crop.x).abs() < 1e-4);
        assert!((shown_origin.y - crop.y).abs() < 1e-4);
        assert!((shown_size.x - crop.width).abs() < 1e-4);
        assert!((shown_size.y - crop.height).abs() < 1e-4);
    }

    /// A crop out of a file cannot produce an infinite layout — see
    /// [`ImageCrop::clamped`](crate::models::ImageCrop::clamped), which this
    /// relies on rather than repeating.
    #[test]
    fn an_absurd_crop_still_lays_out() {
        let (size, offset) = crop_layout(
            Vec2::new(100.0, 100.0),
            ImageCrop {
                x: -5.0,
                y: f32::NAN,
                width: 0.0,
                height: 0.0,
            },
        );

        assert!(size.x.is_finite() && size.y.is_finite(), "{size:?}");
        assert!(offset.x.is_finite() && offset.y.is_finite(), "{offset:?}");
    }
}
