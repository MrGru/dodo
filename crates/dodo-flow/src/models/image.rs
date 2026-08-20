//! §10's embedded raster image: **the bytes, the handle that shares them, and
//! the crop that is metadata rather than a rewrite.**
//!
//! Three types and one rule. The rule is requirements §10's, stated outright:
//!
//! > Do not duplicate raw image bytes per element.
//!
//! So an element carries a [`NodeImage`] — a **handle** and a crop rectangle,
//! sixteen bytes of it — and the bytes themselves live once in
//! [`FlowDocument::images`](crate::models::FlowDocument::images), keyed by that
//! handle. Two elements showing the same picture are two handles pointing at one
//! [`ImageResource`], the Duplicate action copies the handle, and neither can
//! double the memory because neither has a copy to double.
//!
//! # Why the handle is a content hash rather than an id
//!
//! An [`ElementId`](crate::models::ElementId) would have worked and would have
//! been wrong in one case that happens constantly: **the same file inserted
//! twice**. With an allocated id, two inserts of one photograph are two
//! resources and two copies of the bytes, and nothing in the format could ever
//! notice they are identical. With a content hash they collide by construction —
//! [`ImageHandle::of`] hashes the bytes, the second insert finds the entry
//! already there, and sharing is a property of the format rather than a
//! discipline the insert path has to remember.
//!
//! The hash is FNV-1a over the bytes, 64 bits, and it is **not** a security
//! boundary: it identifies content within one document, where a collision would
//! mean two images the same size and the same format hashing alike, and the
//! consequence is one picture drawn in place of another rather than a
//! vulnerability. A cryptographic digest would be a new package in dodo's graph
//! — see [`ids`](crate::models::ids)'s argument against `uuid` — for a
//! stronger guarantee than the thing it guards.
//!
//! # Why the bytes are embedded, and not a path
//!
//! **A flow document is one file, and it stays one file.** The alternative — the
//! document holding a path into the filesystem — was rejected on three counts:
//!
//! 1. A path breaks the moment the document is moved, copied to another machine
//!    or sent to somebody, and it breaks *silently*, months later, with the
//!    picture simply gone.
//! 2. dodo has no asset directory for a canvas document to sit beside. Where a
//!    document lives under `data_dir()` is Phase 8's question and
//!    `docs/architecture/persistence.md` is its authority; a sidecar folder
//!    invented here would be a persistence decision made in the wrong phase.
//! 3. The engine already refuses to serialize derived state, and a path *is*
//!    derived state of a kind — it is a fact about one machine's filesystem
//!    rather than about the diagram.
//!
//! The cost is stated rather than hidden: JSON has no byte string, so the bytes
//! are **base64**, which is four characters for every three bytes — a 33 %
//! surcharge on the compressed file, on top of a document that was previously
//! measured in kilobytes. That is the price of a diagram that opens on another
//! machine, and it is the same trade Excalidraw makes.
//!
//! # The crop is a window on the source, in fractions
//!
//! [`ImageCrop`] is four numbers in `0.0..=1.0`, relative to the source's own
//! size, and the whole of §10's *"cropping adjusts which part of the source is
//! shown; the original bytes are untouched and shared"*. Fractions rather than
//! pixels for two reasons: a crop stays meaningful if the same resource is ever
//! re-encoded at another resolution, and it needs no decode to be read — the
//! panel, the aspect-lock arithmetic and the serializer all work on a crop
//! without ever touching a pixel.
//!
//! **This file names no UI framework**, and it names no image decoder either.
//! What a `Png` actually contains is `views/`'s question; here it is a length
//! and a format tag.

use std::{fmt, sync::Arc};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::geometry::Vec2;

/// The encodings dodo will hand to a decoder.
///
/// One variant per format GPUI's own image path decodes — see
/// `gpui::ImageFormat`, which this deliberately mirrors rather than wraps: this
/// module sits below the UI-framework line, and a `gpui` type in a serialized
/// field would put the document format on the far side of it.
///
/// The tag is what is written to the file, so the names are part of the format
/// and are not free to be renamed for tidiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Ico,
    Tiff,
}

impl ImageFormat {
    pub const ALL: &'static [ImageFormat] = &[
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::Webp,
        ImageFormat::Bmp,
        ImageFormat::Ico,
        ImageFormat::Tiff,
    ];

    /// The format a file extension names, or `None` for anything else.
    ///
    /// **By extension rather than by sniffing the magic bytes**, because the
    /// only caller is a native file picker the user chose a file in: the
    /// extension is what they saw in the dialog, and disagreeing with it would
    /// mean opening a file they did not pick. A decoder that then refuses the
    /// bytes is the honest failure, and it is one the insert path reports.
    pub fn of_extension(extension: &str) -> Option<ImageFormat> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "gif" => ImageFormat::Gif,
            "webp" => ImageFormat::Webp,
            "bmp" => ImageFormat::Bmp,
            "ico" => ImageFormat::Ico,
            "tif" | "tiff" => ImageFormat::Tiff,
            _ => return None,
        })
    }

    /// The extensions a file picker should offer, in one place so the dialog
    /// and [`of_extension`](ImageFormat::of_extension) cannot disagree about
    /// what dodo can open.
    pub const EXTENSIONS: &'static [&'static str] = &[
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff",
    ];

    /// A short stable name, for a test or a trace line. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Gif => "gif",
            ImageFormat::Webp => "webp",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Ico => "ico",
            ImageFormat::Tiff => "tiff",
        }
    }
}

/// **What an element points at instead of holding bytes**: a 64-bit content
/// hash of an [`ImageResource`].
///
/// See the module doc for why it is a hash of the content rather than an
/// allocated id, and why 64 bits of FNV are enough for the job it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageHandle(u64);

impl ImageHandle {
    /// The handle for these bytes. **The only way to make one**, so a resource
    /// cannot be filed under a handle that is not its own.
    pub fn of(bytes: &[u8]) -> ImageHandle {
        // FNV-1a, 64-bit. Written out rather than pulled in: it is four lines,
        // it is deterministic across builds and platforms — which a `HashMap`'s
        // `DefaultHasher` explicitly is not, and a document is written on one
        // machine and read on another — and dodo counts its packages.
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;

        let mut hash = OFFSET;
        for &byte in bytes {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(PRIME);
        }
        ImageHandle(hash)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Rebuilds a handle read from a file. `pub(crate)` on purpose: everything
    /// that *makes* an image goes through [`ImageHandle::of`], and this exists
    /// for the deserializer alone.
    pub(crate) const fn from_raw(raw: u64) -> ImageHandle {
        ImageHandle(raw)
    }
}

impl fmt::Display for ImageHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Written as a hex string rather than as a number, because it is a **map key**
/// in the document and JSON object keys are strings either way — so a numeric
/// handle would be serialized as a quoted decimal that no reader could tell
/// from a count. Sixteen hex digits reads as an identifier.
impl Serialize for ImageHandle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ImageHandle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ImageHandle, D::Error> {
        let text = String::deserialize(deserializer)?;
        u64::from_str_radix(&text, 16)
            .map(ImageHandle::from_raw)
            .map_err(serde::de::Error::custom)
    }
}

/// **One picture's bytes, stored once per document.**
///
/// The pixel dimensions are stored beside them and that is not redundancy: the
/// aspect-ratio lock, the crop arithmetic and the size a freshly inserted image
/// is given all need the source's shape, and every one of them would otherwise
/// have to decode the file to ask. A decode is milliseconds and needs a GPUI
/// `App`; two integers in the document make all three of those pure functions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageResource {
    pub format: ImageFormat,
    /// The source's own width in pixels.
    pub width: u32,
    pub height: u32,
    /// The file, verbatim. `Arc` so the runtime shares one allocation with the
    /// document it was loaded from and with every clone of either.
    #[serde(with = "base64_bytes")]
    pub bytes: Arc<[u8]>,
}

impl ImageResource {
    pub fn new(format: ImageFormat, width: u32, height: u32, bytes: impl Into<Arc<[u8]>>) -> Self {
        ImageResource {
            format,
            width,
            height,
            bytes: bytes.into(),
        }
    }

    /// This resource's handle — the hash of its own bytes.
    pub fn handle(&self) -> ImageHandle {
        ImageHandle::of(&self.bytes)
    }

    /// **The size a freshly inserted picture is given**, in world units:
    /// its own pixel dimensions, shrunk to fit inside `room` and never grown.
    ///
    /// One pixel is one world unit at 100 % zoom, so a screenshot arrives at
    /// the size it was taken — which is the only size that is not a guess. The
    /// fit is what stops a 6,000-pixel photograph arriving ten screens wide,
    /// and *never grown* is what stops a 16-pixel icon being blown up into a
    /// blurry rectangle nobody asked for.
    ///
    /// `room` is normally the viewport in world units, scaled down a little so
    /// the picture lands inside the view with its grips reachable.
    pub fn placed_size(&self, room: Vec2) -> Vec2 {
        let natural = Vec2::new(
            (self.width.max(1) as f32).min(f32::MAX),
            self.height.max(1) as f32,
        );
        let room = Vec2::new(room.x.max(1.0), room.y.max(1.0));

        let scale = (room.x / natural.x).min(room.y / natural.y).min(1.0);
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Vec2::new(natural.x * scale, natural.y * scale)
    }

    /// The source's width divided by its height, or `1.0` for a resource whose
    /// dimensions are missing.
    ///
    /// Never zero and never infinite: a ratio is divided by in the aspect lock,
    /// and a document another build wrote may say anything at all.
    pub fn aspect(&self) -> f32 {
        match (self.width, self.height) {
            (0, _) | (_, 0) => 1.0,
            (width, height) => width as f32 / height as f32,
        }
    }
}

/// **The part of the source an element shows** — §10's crop, as metadata.
///
/// Fractions of the source, `0.0..=1.0`, with the full picture as the default,
/// so an image nobody has cropped costs four numbers that mean "all of it" and
/// serializes to the same thing every uncropped image does.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageCrop {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for ImageCrop {
    fn default() -> ImageCrop {
        ImageCrop::FULL
    }
}

impl ImageCrop {
    /// The whole source.
    pub const FULL: ImageCrop = ImageCrop {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    /// The smallest crop this engine will produce or accept, as a fraction of
    /// each axis. A window narrower than this is a picture nobody can see, and
    /// dividing by it is how a scale factor becomes an infinity.
    pub const MIN_EXTENT: f32 = 0.01;

    pub fn new(x: f32, y: f32, width: f32, height: f32) -> ImageCrop {
        ImageCrop {
            x,
            y,
            width,
            height,
        }
        .clamped()
    }

    /// Whether this shows the whole picture. The Crop action reads it, and so
    /// does the renderer — an uncropped image is painted without the extra
    /// arithmetic and without the clip that goes with it.
    pub fn is_full(self) -> bool {
        let full = self.clamped();
        full.x <= 0.0 && full.y <= 0.0 && full.width >= 1.0 && full.height >= 1.0
    }

    /// **Every crop that reaches the renderer has been through this.** A file
    /// may carry anything — a negative origin, a window running off the right
    /// edge, a `NaN` — and a crop is divided by, so an unclamped one is a
    /// division by zero inside a paint.
    pub fn clamped(self) -> ImageCrop {
        let sane = |value: f32, fallback: f32| if value.is_finite() { value } else { fallback };

        let width = sane(self.width, 1.0).clamp(ImageCrop::MIN_EXTENT, 1.0);
        let height = sane(self.height, 1.0).clamp(ImageCrop::MIN_EXTENT, 1.0);
        let x = sane(self.x, 0.0).clamp(0.0, 1.0 - width);
        let y = sane(self.y, 0.0).clamp(0.0, 1.0 - height);

        ImageCrop {
            x,
            y,
            width,
            height,
        }
    }

    /// The shown window's width over its height, **given the source's own
    /// aspect** — which is what the frame has to match for the picture not to
    /// be stretched.
    pub fn aspect(self, source: f32) -> f32 {
        let crop = self.clamped();
        let ratio = source * (crop.width / crop.height);
        if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else {
            1.0
        }
    }

    /// **The centred window of `target` aspect inside this one** — the whole of
    /// the Crop action's arithmetic, and pure.
    ///
    /// `source` is the resource's own width/height ratio and `target` is the
    /// frame's. The result is the largest sub-window of the current crop whose
    /// shown aspect is `target`, centred on what is shown now, so cropping
    /// keeps the middle of the picture rather than its top-left corner.
    ///
    /// It composes: cropping to a frame and then cropping to the same frame
    /// again changes nothing, because the second call's largest window is the
    /// whole of the first's.
    pub fn cropped_to_aspect(self, source: f32, target: f32) -> ImageCrop {
        let crop = self.clamped();
        if !(source.is_finite() && source > 0.0) || !(target.is_finite() && target > 0.0) {
            return crop;
        }

        // The shown window's aspect, and how far it is from the one asked for.
        // A shown window that is too wide loses width; one that is too tall
        // loses height. Exactly one of the two branches runs.
        let shown = crop.aspect(source);
        let scale = shown / target;

        if (scale - 1.0).abs() < 1e-4 {
            return crop;
        }

        if scale > 1.0 {
            let width = crop.width / scale;
            ImageCrop {
                x: crop.x + (crop.width - width) * 0.5,
                width,
                ..crop
            }
            .clamped()
        } else {
            let height = crop.height * scale;
            ImageCrop {
                y: crop.y + (crop.height - height) * 0.5,
                height,
                ..crop
            }
            .clamped()
        }
    }
}

/// **What an image element carries**: which picture, and how much of it.
///
/// Sixteen bytes and no allocation, so it sits in a document node and in the
/// runtime's cold row without either growing a heap indirection. The bytes are
/// [`FlowDocument::images`](crate::models::FlowDocument::images)'s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NodeImage {
    pub handle: ImageHandle,
    #[serde(default)]
    pub crop: ImageCrop,
}

impl NodeImage {
    /// The whole of a picture.
    pub fn new(handle: ImageHandle) -> NodeImage {
        NodeImage {
            handle,
            crop: ImageCrop::FULL,
        }
    }

    pub fn with_crop(mut self, crop: ImageCrop) -> NodeImage {
        self.crop = crop.clamped();
        self
    }
}

/// Base64, for the one field in this format that is not text.
///
/// Hand-written rather than a dependency, and the reason is dodo's own: every
/// package in the graph is argued for in `deny.toml` and
/// `THIRD-PARTY-NOTICES.md`, and this is thirty lines of table lookup with a
/// round-trip property test beside it. Standard alphabet with padding (RFC
/// 4648 §4), which is what every other tool writing this field would expect.
mod base64_bytes {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serializer};

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let packed = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(ALPHABET[(packed >> 18) as usize & 63] as char);
            out.push(ALPHABET[(packed >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(packed >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[packed as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    /// `None` for anything that is not valid base64. A malformed resource is
    /// reported as a load error rather than repaired: guessing at half a
    /// picture is worse than saying the file is broken.
    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let value = |byte: u8| -> Option<u32> {
            Some(match byte {
                b'A'..=b'Z' => (byte - b'A') as u32,
                b'a'..=b'z' => (byte - b'a') as u32 + 26,
                b'0'..=b'9' => (byte - b'0') as u32 + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            })
        };

        let text = text.trim_end_matches('=').as_bytes();
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        for chunk in text.chunks(4) {
            if chunk.len() == 1 {
                return None;
            }
            let mut packed = 0u32;
            for (index, &byte) in chunk.iter().enumerate() {
                packed |= value(byte)? << (18 - 6 * index);
            }
            out.push((packed >> 16) as u8);
            if chunk.len() > 2 {
                out.push((packed >> 8) as u8);
            }
            if chunk.len() > 3 {
                out.push(packed as u8);
            }
        }
        Some(out)
    }

    pub fn serialize<S: Serializer>(bytes: &Arc<[u8]>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<[u8]>, D::Error> {
        let text = String::deserialize(deserializer)?;
        decode(&text)
            .map(Arc::from)
            .ok_or_else(|| serde::de::Error::custom("image data is not valid base64"))
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageCrop, ImageFormat, ImageHandle, ImageResource, NodeImage, base64_bytes};

    #[test]
    fn the_same_bytes_always_hash_to_the_same_handle() {
        // The property the whole sharing rule rests on: inserting one file
        // twice must produce one resource, and the format is what makes that
        // true rather than the insert path remembering to check.
        let first = ImageHandle::of(b"a picture");
        let second = ImageHandle::of(b"a picture");
        let other = ImageHandle::of(b"a different picture");

        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    #[test]
    fn a_handle_round_trips_as_sixteen_hex_digits() {
        let handle = ImageHandle::of(b"png bytes");
        let json = serde_json::to_string(&handle).unwrap();

        assert_eq!(json.len(), 18, "sixteen digits and two quotes: {json}");
        assert_eq!(
            serde_json::from_str::<ImageHandle>(&json).unwrap(),
            handle,
            "a handle that does not survive a round trip loses its picture"
        );
    }

    #[test]
    fn base64_round_trips_every_length_class() {
        // The three residues are the whole of base64's difficulty: 0, 1 and 2
        // bytes past a multiple of three each pad differently.
        for length in 0..32usize {
            let bytes: Vec<u8> = (0..length).map(|index| (index * 7 + 3) as u8).collect();
            let encoded = base64_bytes::encode(&bytes);
            assert_eq!(encoded.len() % 4, 0, "{length} encodes unpadded");
            assert_eq!(
                base64_bytes::decode(&encoded).as_deref(),
                Some(bytes.as_slice()),
                "length {length}"
            );
        }
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        // Pinned against a known vector, because an encoder that round-trips
        // with its own decoder can still be wrong for every other reader.
        assert_eq!(base64_bytes::encode(b"dodo"), "ZG9kbw==");
        assert_eq!(base64_bytes::decode("ZG9kbw==").unwrap(), b"dodo");
        assert_eq!(base64_bytes::decode("not base64!"), None);
    }

    #[test]
    fn a_resource_round_trips_with_its_bytes_intact() {
        let resource = ImageResource::new(ImageFormat::Png, 800, 600, vec![0u8, 1, 2, 3, 255]);

        let json = serde_json::to_string(&resource).unwrap();
        let back: ImageResource = serde_json::from_str(&json).unwrap();

        assert_eq!(back, resource);
        assert_eq!(back.aspect(), 800.0 / 600.0);
    }

    #[test]
    fn a_resource_with_no_dimensions_still_answers_an_aspect() {
        // A ratio is divided by. A zero here would reach the aspect lock as an
        // infinity and put a node's size past every clamp in the engine.
        let resource = ImageResource::new(ImageFormat::Png, 0, 0, vec![1]);
        assert_eq!(resource.aspect(), 1.0);
    }

    #[test]
    fn a_big_picture_is_shrunk_to_fit_and_a_small_one_is_left_alone() {
        use crate::geometry::Vec2;

        let room = Vec2::new(800.0, 600.0);

        // Four thousand pixels wide, in an 800-unit view: it fits the width and
        // keeps its shape.
        let big = ImageResource::new(ImageFormat::Png, 4000, 2000, vec![1]);
        let size = big.placed_size(room);
        assert!((size.x - 800.0).abs() < 1e-3, "{size:?}");
        assert!((size.y - 400.0).abs() < 1e-3, "{size:?}");

        // A small one arrives at its own size rather than being blown up.
        let small = ImageResource::new(ImageFormat::Png, 64, 64, vec![1]);
        assert_eq!(small.placed_size(room), Vec2::new(64.0, 64.0));

        // And a resource with no dimensions still produces something placeable.
        let broken = ImageResource::new(ImageFormat::Png, 0, 0, vec![1]);
        let size = broken.placed_size(room);
        assert!(size.x > 0.0 && size.y > 0.0, "{size:?}");
    }

    #[test]
    fn a_crop_is_clamped_into_the_source() {
        let crop = ImageCrop::new(-0.5, 0.9, 2.0, 0.5);

        assert!(crop.x >= 0.0 && crop.y >= 0.0);
        assert!(crop.x + crop.width <= 1.0 + 1e-6);
        assert!(crop.y + crop.height <= 1.0 + 1e-6);
    }

    #[test]
    fn an_absurd_crop_from_a_file_cannot_produce_an_infinity() {
        let crop = ImageCrop {
            x: f32::NAN,
            y: f32::INFINITY,
            width: 0.0,
            height: -3.0,
        }
        .clamped();

        assert!(crop.width >= ImageCrop::MIN_EXTENT);
        assert!(crop.height >= ImageCrop::MIN_EXTENT);
        assert!(crop.aspect(1.0).is_finite());
    }

    #[test]
    fn the_default_crop_is_the_whole_picture() {
        assert!(ImageCrop::default().is_full());
        assert!(!ImageCrop::new(0.1, 0.1, 0.5, 0.5).is_full());
        assert_eq!(NodeImage::new(ImageHandle::of(b"x")).crop, ImageCrop::FULL);
    }

    #[test]
    fn cropping_to_an_aspect_keeps_the_middle_and_is_idempotent() {
        // A 2:1 source shown in a 1:1 frame: half the width goes, a quarter
        // from each side, and the picture is not moved off centre.
        let cropped = ImageCrop::FULL.cropped_to_aspect(2.0, 1.0);

        assert!((cropped.width - 0.5).abs() < 1e-5, "{cropped:?}");
        assert!((cropped.x - 0.25).abs() < 1e-5, "{cropped:?}");
        assert!((cropped.height - 1.0).abs() < 1e-5);
        assert!((cropped.aspect(2.0) - 1.0).abs() < 1e-4);

        // Twice is once: the second call's largest window is the whole of the
        // first's, so a user pressing Crop again does not walk the picture in.
        let again = cropped.cropped_to_aspect(2.0, 1.0);
        assert!((again.width - cropped.width).abs() < 1e-5, "{again:?}");
        assert!((again.x - cropped.x).abs() < 1e-5);
    }

    #[test]
    fn cropping_a_wide_frame_out_of_a_tall_source_loses_height() {
        let cropped = ImageCrop::FULL.cropped_to_aspect(0.5, 2.0);

        assert!((cropped.width - 1.0).abs() < 1e-5, "{cropped:?}");
        assert!((cropped.height - 0.25).abs() < 1e-5, "{cropped:?}");
        assert!((cropped.aspect(0.5) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn a_format_is_recognised_by_extension_in_any_case() {
        assert_eq!(ImageFormat::of_extension("PNG"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::of_extension("jpeg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::of_extension("svg"), None);

        // The picker's list and the reader's table are one fact stated twice,
        // so they are checked against each other rather than trusted.
        for extension in ImageFormat::EXTENSIONS {
            assert!(
                ImageFormat::of_extension(extension).is_some(),
                "the picker offers {extension} and the reader refuses it"
            );
        }
    }
}
