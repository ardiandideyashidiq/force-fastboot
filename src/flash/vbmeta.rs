/// Vendored empty vbmeta image generated with:
/// `avbtool make_vbmeta_image --flags 3 --output vendor/empty_vbmeta.img`
///
/// Flags = 3 (`HASHTREE_DISABLED` | `VERIFICATION_DISABLED`).
pub const EMPTY_VBMETA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/empty_vbmeta.img"
));
