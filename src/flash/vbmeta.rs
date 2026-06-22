/// Vendored empty vbmeta image with AVB flags=3:
///   - bit 0: `HASHTREE_DISABLED` (disable dm-verity)
///   - bit 1: `VERIFICATION_DISABLED` (disable AVB verification)
pub const EMPTY_VBMETA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/empty_vbmeta.img"
));
