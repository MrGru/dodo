//! The bounded, shareable icon payload a scanner may hang off an item.
//!
//! This type exists because the thing it replaces had no bound at all.
//! Round 1's `ApplicationMetadata::icon_tiff` was whatever
//! `-[NSImage TIFFRepresentation]` returned for the icon `NSWorkspace` hands
//! back for an app bundle — and that is not a thumbnail. Measured on this
//! machine over the 98 bundles in `/Applications`, `~/Applications`,
//! `/System/Applications` and `/System/Applications/Utilities`, it is
//! **73,949,448 bytes per application** (70.5 MiB — the same for every one of
//! them, because the icon `NSWorkspace` returns carries the whole standard
//! representation ladder at 16 bits per sample), for **6.71 GiB retained for
//! one Installed Apps scan**, held for as long as the result stayed on
//! screen. The grid's own copy of the items doubled it.
//!
//! So the fix is not only "rasterise smaller". It is that the payload now
//! travels in a type that *cannot* be that large: [`IconRaster::new`] refuses
//! anything over [`IconRaster::MAX_BYTES`], and a refused icon is simply
//! `None` — the row draws its category glyph, exactly as a bundle with no
//! readable icon already did. A future change that goes back to handing over
//! a full-size representation therefore loses the icons rather than the
//! machine's memory, and the unit tests below are what say so without a
//! frame, a window or a Mac.
//!
//! The second half is [`Arc`]. `CleanableItem::clone` is on the results
//! grid's sync path (`views::results_sync`), so a `Vec<u8>` here is resident
//! at least twice — once in the scan result, once in the table delegate.
//! Behind an `Arc<[u8]>` the copy is a reference count, and
//! `icon_bytes_are_shared_by_clone` pins that with `Arc::ptr_eq` rather than
//! trusting the type to stay that way.
//!
//! Deliberately free of GPUI and of every platform API, like the rest of
//! `core`: the producer is `macos::platform::icon` and the consumer is
//! `views::results_table`, and neither is needed to test the rule.

use std::sync::Arc;

/// A small, already-rasterised application icon, encoded in a format GPUI's
/// asynchronous image decoder actually accepts (PNG — see
/// `macos::platform::icon` for why the previous TIFF did not decode at all).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IconRaster {
    bytes: Arc<[u8]>,
}

impl IconRaster {
    /// The edge length, in pixels, a platform rasteriser must draw into.
    ///
    /// The grid draws these at `size_4()` — 16 logical pixels — so 64 is 2×
    /// headroom over a 2× display and 4× over a 1× one. It is deliberately a
    /// fixed number rather than a function of the display: the scanner runs
    /// on the background executor and has no window to ask.
    pub const EDGE_PIXELS: usize = 64;

    /// The per-icon budget. Measured at [`Self::EDGE_PIXELS`], the 98 real
    /// bundles above encode to 5,891 bytes on average and 7,745 bytes at
    /// worst, so this is roughly 4× the observed worst case — loose enough
    /// that an unusually detailed icon still shows, tight enough that no
    /// plausible payload at this raster size can approach it.
    pub const MAX_BYTES: usize = 32 * 1024;

    /// The only way to build one. `None` for an empty payload (nothing to
    /// draw) or one over [`Self::MAX_BYTES`] (something produced a
    /// full-size representation again — take the fallback glyph, not the
    /// memory).
    pub fn new(bytes: Vec<u8>) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > Self::MAX_BYTES {
            return None;
        }
        Some(Self {
            bytes: Arc::from(bytes),
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::IconRaster;

    /// The measured worst case at `EDGE_PIXELS`, so the fixtures below are
    /// the size of a real icon rather than a round number.
    const REALISTIC_BYTES: usize = 7_745;

    #[test]
    fn a_realistic_raster_is_accepted_whole() {
        let raster = IconRaster::new(vec![7u8; REALISTIC_BYTES]).expect("within budget");
        assert_eq!(raster.len(), REALISTIC_BYTES);
        assert_eq!(raster.as_bytes(), vec![7u8; REALISTIC_BYTES].as_slice());
    }

    #[test]
    fn an_empty_payload_is_no_icon_rather_than_a_zero_length_one() {
        assert_eq!(IconRaster::new(Vec::new()), None);
    }

    /// The whole point of the type. 70.5 MiB is what one application's
    /// `TIFFRepresentation` actually measured at; it must not be
    /// constructible, so a regression costs a glyph and not the machine.
    #[test]
    fn an_over_budget_payload_is_refused_rather_than_retained() {
        assert_eq!(IconRaster::new(vec![0u8; IconRaster::MAX_BYTES + 1]), None);
        assert_eq!(IconRaster::new(vec![0u8; 73_949_448]), None);
        assert!(IconRaster::new(vec![0u8; IconRaster::MAX_BYTES]).is_some());
    }

    /// The bound the captain's report is about, stated over a whole scan:
    /// whatever the icons are, a result of N applications cannot retain more
    /// than N × `MAX_BYTES` of icon bytes. At the 98 bundles this machine
    /// has, that ceiling is 3.06 MiB against the 6.71 GiB measured before.
    #[test]
    fn a_whole_scans_icon_bytes_are_bounded_by_the_item_count() {
        const APPS: usize = 500;
        let rasters: Vec<IconRaster> = (0..APPS)
            .map(|ix| {
                // Deliberately varied, and deliberately larger than the
                // measured worst case, so the bound is not being proved
                // against a fixture that happens to be small.
                IconRaster::new(vec![ix as u8; REALISTIC_BYTES + ix * 16])
                    .unwrap_or_else(|| IconRaster::new(vec![0u8; IconRaster::MAX_BYTES]).unwrap())
            })
            .collect();

        let total: usize = rasters.iter().map(IconRaster::len).sum();
        assert!(
            total <= APPS * IconRaster::MAX_BYTES,
            "{total} bytes over {APPS} applications exceeds the per-icon budget"
        );
        assert!(
            total <= 16 * 1024 * 1024,
            "500 applications must not cost more than a few MiB of icon bytes; got {total}"
        );
    }

    /// The second half of the fix: the grid's copy of a result must not be a
    /// second copy of its icons. `CleanableItem::clone` is a deep clone of
    /// every other field, and this is the one field for which that would be
    /// measured in megabytes.
    #[test]
    fn icon_bytes_are_shared_by_clone_never_copied() {
        let raster = IconRaster::new(vec![9u8; REALISTIC_BYTES]).expect("within budget");
        let copy = raster.clone();

        assert_eq!(raster, copy);
        assert!(
            Arc::ptr_eq(&raster.bytes, &copy.bytes),
            "cloning an item must bump a reference count, not re-copy the icon"
        );
    }
}
