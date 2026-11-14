//! The clipboard the apps' copy and paste go through: plain text plus the rich forms other
//! programs (and the other tabs) read (HTML, RTF, MathML, pictures), and a change token, so
//! each app can tell whether the clipboard still holds *its own* last copy: every tab's app
//! lives in this one process, so "this process copied it" isn't enough.
//!
//! [`SystemClipboard`] is the real one (macOS: `NSPasteboard` through objc2's safe calls, as the
//! Craft apps do; pictures through `arboard`). [`MemoryClipboard`] keeps it in the process,
//! for tests and `--check`, which must not touch the user's clipboard.

use std::sync::Mutex;

/// One copy, in every form it was offered in. Empty strings are left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copy {
    pub text: String,
    pub html: String,
    pub rtf: String,
    pub mathml: String,
    /// A picture, PNG-encoded.
    pub png: Vec<u8>,
}

/// What the clipboard holds and can be asked for.
pub trait Clipboard: Send + Sync {
    fn text(&self) -> Option<String>;
    fn html(&self) -> Option<String>;
    fn rtf(&self) -> Option<String>;
    /// The picture, PNG-encoded (another program's TIFF or PNG).
    fn picture_png(&self) -> Option<Vec<u8>>;
    /// The names of the formats on the clipboard.
    fn formats(&self) -> Vec<String>;
    /// Replaces the clipboard with `copy`: the change token it now has, when its text (and
    /// HTML, if any) are there.
    fn set(&self, copy: &Copy) -> Option<i64>;
    /// The clipboard's change token: it changes with every copy, ours or another program's.
    fn token(&self) -> i64;
}

/// One app's view of the clipboard: its copies, and whether the clipboard still holds its
/// own last one (each app keeps its own [`Owner`]).
#[derive(Debug, Default)]
pub struct Owner {
    last: Mutex<Option<i64>>,
}

impl Owner {
    pub fn set(&self, clipboard: &dyn Clipboard, copy: &Copy) -> bool {
        let token = clipboard.set(copy);
        if let Ok(mut g) = self.last.lock() {
            *g = token;
        }
        token.is_some()
    }

    pub fn is_own(&self, clipboard: &dyn Clipboard) -> bool {
        self.last.lock().ok().and_then(|g| *g) == Some(clipboard.token())
    }
}

/// Caps on what a paste reads (input from other programs is untrusted, as the Craft apps
/// treat it): 64 MB of text or markup, 256 MB of picture.
const MAX_TEXT: usize = 64 << 20;
const MAX_PICTURE: usize = 256 << 20;

fn capped(s: String) -> Option<String> {
    (s.len() <= MAX_TEXT && !s.is_empty()).then_some(s)
}

/// An in-process clipboard (tests, `--check`).
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    inner: Mutex<(Copy, i64)>,
}

impl MemoryClipboard {
    /// What another program would have put there (not this process's own copy).
    pub fn put_foreign(&self, copy: Copy) {
        if let Ok(mut g) = self.inner.lock() {
            let next = g.1.wrapping_add(1);
            *g = (copy, next);
        }
    }

    pub fn contents(&self) -> Copy {
        self.inner.lock().map(|g| g.0.clone()).unwrap_or_default()
    }
}

impl Clipboard for MemoryClipboard {
    fn text(&self) -> Option<String> {
        capped(self.contents().text)
    }
    fn html(&self) -> Option<String> {
        capped(self.contents().html)
    }
    fn rtf(&self) -> Option<String> {
        capped(self.contents().rtf)
    }
    fn picture_png(&self) -> Option<Vec<u8>> {
        Some(self.contents().png).filter(|p| !p.is_empty() && p.len() <= MAX_PICTURE)
    }
    fn formats(&self) -> Vec<String> {
        let c = self.contents();
        [("text", !c.text.is_empty()), ("html", !c.html.is_empty()), ("rtf", !c.rtf.is_empty()), ("png", !c.png.is_empty())]
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(n, _)| n.to_string())
            .collect()
    }
    fn set(&self, copy: &Copy) -> Option<i64> {
        let mut g = self.inner.lock().ok()?;
        let next = g.1.wrapping_add(1);
        *g = (copy.clone(), next);
        Some(next)
    }
    fn token(&self) -> i64 {
        self.inner.lock().map(|g| g.1).unwrap_or(-1)
    }
}

/// The system clipboard.
#[derive(Debug, Default)]
pub struct SystemClipboard;

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSData, NSString};

    pub const TEXT: &str = "public.utf8-plain-text";
    pub const HTML: &str = "public.html";
    pub const RTF: &str = "public.rtf";
    pub const PNG: &str = "public.png";
    /// The names W3C MathML 3 registers for transferring MathML (and the ones word processors
    /// read a pasted equation from first).
    pub const MATHML: [&str; 2] = ["public.mathml.presentation", "public.mathml"];

    pub fn string(ty: &str) -> Option<String> {
        let board = NSPasteboard::generalPasteboard();
        if let Some(s) = board.stringForType(&NSString::from_str(ty)) {
            return Some(s.to_string());
        }
        data(ty).map(|d| String::from_utf8_lossy(&d).into_owned())
    }

    pub fn data(ty: &str) -> Option<Vec<u8>> {
        NSPasteboard::generalPasteboard().dataForType(&NSString::from_str(ty)).map(|d| d.to_vec())
    }

    pub fn types() -> Vec<String> {
        NSPasteboard::generalPasteboard().types().map(|a| a.iter().map(|t| t.to_string()).collect()).unwrap_or_default()
    }

    /// Replaces the pasteboard's contents; the change count after it.
    pub fn set(copy: &super::Copy) -> (bool, isize) {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        let mut ok = board.setString_forType(&NSString::from_str(&copy.text), &NSString::from_str(TEXT));
        if !copy.html.is_empty() {
            ok &= board.setString_forType(&NSString::from_str(&copy.html), &NSString::from_str(HTML));
        }
        if !copy.rtf.is_empty() && !board.setData_forType(Some(&NSData::with_bytes(copy.rtf.as_bytes())), &NSString::from_str(RTF)) {
            log::debug!("the pasteboard refused the RTF");
        }
        if !copy.mathml.is_empty() {
            for name in MATHML {
                let _ = board.setString_forType(&NSString::from_str(&copy.mathml), &NSString::from_str(name));
            }
        }
        if !copy.png.is_empty() {
            let _ = board.setData_forType(Some(&NSData::with_bytes(&copy.png)), &NSString::from_str(PNG));
        }
        (ok, board.changeCount())
    }

    pub fn change_count() -> isize {
        NSPasteboard::generalPasteboard().changeCount()
    }
}

impl SystemClipboard {
    fn arboard_png() -> Option<Vec<u8>> {
        let mut cb = arboard::Clipboard::new().ok()?;
        let img = cb.get_image().ok()?;
        if img.bytes.len() > MAX_PICTURE {
            return None;
        }
        let rgba = image::RgbaImage::from_raw(u32::try_from(img.width).ok()?, u32::try_from(img.height).ok()?, img.bytes.into_owned())?;
        let mut out = std::io::Cursor::new(Vec::new());
        rgba.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(out.into_inner())
    }
}

impl Clipboard for SystemClipboard {
    #[cfg(target_os = "macos")]
    fn text(&self) -> Option<String> {
        mac::string(mac::TEXT).and_then(capped)
    }
    #[cfg(target_os = "macos")]
    fn html(&self) -> Option<String> {
        mac::string(mac::HTML).and_then(capped)
    }
    #[cfg(target_os = "macos")]
    fn rtf(&self) -> Option<String> {
        mac::data(mac::RTF).map(|d| String::from_utf8_lossy(&d).into_owned()).and_then(capped)
    }
    #[cfg(target_os = "macos")]
    fn formats(&self) -> Vec<String> {
        mac::types()
    }
    #[cfg(target_os = "macos")]
    fn set(&self, copy: &Copy) -> Option<i64> {
        let (ok, count) = mac::set(copy);
        ok.then(|| i64::try_from(count).unwrap_or(-1))
    }
    #[cfg(target_os = "macos")]
    fn token(&self) -> i64 {
        i64::try_from(mac::change_count()).unwrap_or(-1)
    }

    #[cfg(not(target_os = "macos"))]
    fn text(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok().and_then(capped)
    }
    #[cfg(not(target_os = "macos"))]
    fn html(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get().html().ok().and_then(capped)
    }
    #[cfg(not(target_os = "macos"))]
    fn rtf(&self) -> Option<String> {
        None
    }
    #[cfg(not(target_os = "macos"))]
    fn formats(&self) -> Vec<String> {
        Vec::new()
    }
    #[cfg(not(target_os = "macos"))]
    fn set(&self, copy: &Copy) -> Option<i64> {
        let mut cb = arboard::Clipboard::new().ok()?;
        let ok = if copy.html.is_empty() {
            cb.set_text(copy.text.clone()).is_ok()
        } else {
            cb.set_html(copy.html.clone(), Some(copy.text.clone())).is_ok()
        };
        // No change count here: never "still ours", so the apps read what's there.
        ok.then_some(-2)
    }
    #[cfg(not(target_os = "macos"))]
    fn token(&self) -> i64 {
        -3
    }

    fn picture_png(&self) -> Option<Vec<u8>> {
        #[cfg(target_os = "macos")]
        if let Some(png) = mac::data(mac::PNG).filter(|p| p.len() <= MAX_PICTURE) {
            return Some(png);
        }
        Self::arboard_png()
    }
}

/// A picture's pixels, PNG-encoded.
pub fn png_from_rgba(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(u32::try_from(width).ok()?, u32::try_from(height).ok()?, rgba.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// A PNG's pixels.
pub fn rgba_from_png(png: &[u8]) -> Option<image::RgbaImage> {
    image::load_from_memory_with_format(png, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_clipboard_keeps_every_form() {
        let c = MemoryClipboard::default();
        assert!(c.text().is_none());
        assert!(c.set(&Copy { text: "Hi".into(), html: "<b>Hi</b>".into(), ..Default::default() }).is_some());
        assert_eq!(c.text().as_deref(), Some("Hi"));
        assert_eq!(c.html().as_deref(), Some("<b>Hi</b>"));
        assert!(c.rtf().is_none(), "empty forms aren't offered");
        assert_eq!(c.formats(), vec!["text".to_string(), "html".to_string()]);
    }

    /// Two apps in one process: each knows whether the clipboard holds *its* last copy.
    #[test]
    fn each_app_knows_its_own_copy() {
        let c = MemoryClipboard::default();
        let (word, sheet) = (Owner::default(), Owner::default());
        assert!(word.set(&c, &Copy { text: "from word".into(), ..Default::default() }));
        assert!(word.is_own(&c) && !sheet.is_own(&c));
        assert!(sheet.set(&c, &Copy { text: "from sheet".into(), ..Default::default() }));
        assert!(!word.is_own(&c), "the sheet's copy isn't the document's");
        assert!(sheet.is_own(&c));
        c.put_foreign(Copy { text: "another program".into(), ..Default::default() });
        assert!(!sheet.is_own(&c));
    }

    #[test]
    fn pictures_round_trip_through_png() {
        let rgba = vec![255u8, 0, 0, 255, 0, 255, 0, 255];
        let png = png_from_rgba(2, 1, &rgba).unwrap();
        let back = rgba_from_png(&png).unwrap();
        assert_eq!((back.width(), back.height()), (2, 1));
        assert_eq!(back.into_raw(), rgba);
        assert!(png_from_rgba(3, 3, &rgba).is_none(), "the size must match the pixels");
    }
}
