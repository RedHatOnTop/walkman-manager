use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use image::codecs::jpeg::JpegEncoder;

use super::hex_id;

/// Long edge of a cached cover. Album tiles are at most 220 css px wide on a 2x screen.
const SIZE: u32 = 440;
/// Cover images larger than this are not decoded.
const MAX_SOURCE: u64 = 40 * 1024 * 1024;

/// Cover thumbnails as JPEG files named by the hash of the source image, so one
/// cover embedded in every track of an album is decoded once.
pub struct ArtCache {
    dir: PathBuf,
    tmp: AtomicUsize,
}

impl ArtCache {
    pub fn new(dir: PathBuf) -> Self {
        ArtCache {
            dir,
            tmp: AtomicUsize::new(0),
        }
    }

    /// The cached file for a key, if the key is well formed and the file exists.
    pub fn path(&self, key: &str) -> Option<PathBuf> {
        let path = self.file(key)?;
        path.is_file().then_some(path)
    }

    fn file(&self, key: &str) -> Option<PathBuf> {
        let valid = key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit());
        valid.then(|| self.dir.join(format!("{key}.jpg")))
    }

    pub fn store(&self, bytes: &[u8]) -> Option<String> {
        self.store_as(&hex_id(bytes), bytes)
    }

    /// Caches an image under a key the caller chose, for covers named by where they
    /// came from rather than by their bytes.
    pub fn store_as(&self, key: &str, bytes: &[u8]) -> Option<String> {
        let path = self.file(key)?;
        if path.is_file() {
            return Some(key.to_owned());
        }
        let thumb = image::load_from_memory(bytes).ok()?.thumbnail(SIZE, SIZE).to_rgb8();
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 86).encode_image(&thumb).ok()?;
        fs::create_dir_all(&self.dir).ok()?;
        let n = self.tmp.fetch_add(1, Ordering::Relaxed);
        let tmp = self.dir.join(format!(".{key}.{}.{n}.tmp", std::process::id()));
        fs::write(&tmp, &out).ok()?;
        if fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
            return None;
        }
        Some(key.to_owned())
    }

    pub fn store_file(&self, path: &Path) -> Option<String> {
        if fs::metadata(path).ok()?.len() > MAX_SOURCE {
            return None;
        }
        self.store(&fs::read(path).ok()?)
    }
}

/// Folder images Sony's and most other players look for, best first.
pub const FOLDER_IMAGES: [&str; 8] = [
    "cover.jpg",
    "folder.jpg",
    "front.jpg",
    "cover.png",
    "folder.png",
    "front.png",
    "cover.jpeg",
    "albumart.jpg",
];

/// The best folder image among a directory's file names, compared without case.
pub fn folder_image<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names
        .into_iter()
        .filter_map(|n| {
            let lower = n.to_lowercase();
            FOLDER_IMAGES.iter().position(|f| *f == lower).map(|rank| (rank, n))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, n)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_folder_images_by_rank() {
        assert_eq!(folder_image(["01.flac", "Folder.jpg", "Cover.JPG"]), Some("Cover.JPG"));
        assert_eq!(folder_image(["back.jpg", "scan.png"]), None);
    }

    #[test]
    fn thumbnails_are_cached_by_content() {
        let dir = std::env::temp_dir().join(format!("uwm-art-test-{}", std::process::id()));
        let cache = ArtCache::new(dir.clone());
        let img = image::RgbImage::from_fn(900, 600, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let key = cache.store(&png).unwrap();
        assert_eq!(cache.store(&png).as_deref(), Some(key.as_str()));
        let thumb = image::open(cache.path(&key).unwrap()).unwrap();
        assert_eq!(thumb.width(), 440);
        assert!((293..=294).contains(&thumb.height()));
        assert!(cache.path("../../etc/passwd").is_none());
        assert_eq!(
            cache.store_as("00000000000000aa", &png).as_deref(),
            Some("00000000000000aa")
        );
        assert!(cache.path("00000000000000aa").is_some());
        assert!(cache.store_as("../x", &png).is_none());
        let _ = fs::remove_dir_all(dir);
    }
}
