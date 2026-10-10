//! Brand asset integrity (Phase 4): the committed icon files exist, the
//! ICO parses as multi-image Windows icon, and each PNG carries the
//! expected dimensions. Pure-stdlib parsing — no image dependencies.

use std::path::{Path, PathBuf};

fn brand_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("brand")
}

fn read_png_dimensions(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).expect("read png");
    assert!(
        bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]),
        "{} must start with the PNG signature",
        path.display()
    );
    assert!(
        &bytes[12..16] == b"IHDR",
        "{} first chunk must be IHDR",
        path.display()
    );
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

#[test]
fn icon_png_exports_have_expected_dimensions() {
    for size in [512u32, 256, 128, 64, 32, 16] {
        let path = brand_dir().join(format!("terror-bats-icon-{size}.png"));
        assert!(path.is_file(), "missing {}", path.display());
        let (w, h) = read_png_dimensions(&path);
        assert_eq!(
            (w, h),
            (size, size),
            "wrong dimensions in {}",
            path.display()
        );
        let bytes = std::fs::read(&path).expect("read png");
        assert!(!bytes.is_empty());
    }
}

#[test]
fn windows_ico_is_multi_image() {
    let path = brand_dir().join("terrorbats.ico");
    let bytes = std::fs::read(&path).expect("read ico");
    assert!(bytes.len() > 6, "ico too small");
    assert_eq!(&bytes[0..4], &[0, 0, 1, 0], "ico header must be ICO type 1");
    let count = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    assert!(count >= 3, "ico must bundle several sizes, saw {count}");
}
