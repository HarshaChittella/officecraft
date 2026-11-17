//! Each app's `Services`, filled in from the [`Platform`]. The callbacks follow each app's
//! documented contract (its `services.rs`); the app decides when to call them.

use crate::clipboard::{Copy, Owner, png_from_rgba, rgba_from_png};
use crate::{Platform, files, print};
use officecraft_core::engines::{DeckEditor, PdfEditor, SheetEditor, WordEditor};
use officecraft_core::{Editor, Kind};
use std::sync::Arc;

/// An engine of `kind` with its services filled in from `platform`.
pub fn editor(kind: Kind, platform: &Platform) -> Box<dyn Editor> {
    match kind {
        Kind::Document => Box::new(WordEditor::with_services(word_services(platform))),
        Kind::Sheet => Box::new(SheetEditor::with_services(sheet_services(platform))),
        Kind::Presentation => Box::new(DeckEditor::with_services(deck_services(platform))),
        // PdfCraft has no services: it uses its own panels, clipboard and printing (D12).
        Kind::Pdf => Box::new(PdfEditor::new()),
    }
}

/// WordCraft's services (`wordcraft_ui_egui::Services`).
pub fn word_services(p: &Platform) -> wordcraft_ui_egui::Services {
    use wordcraft_ui_egui::paste_picture::ClipboardPicture;
    let cb = p.clipboard.clone();
    let (c1, c2, c3, c4, c5, c6, c7) = (cb.clone(), cb.clone(), cb.clone(), cb.clone(), cb.clone(), cb.clone(), cb);
    let owner = Arc::new(Owner::default());
    let (o1, o2, o3) = (owner.clone(), owner.clone(), owner);
    let interactive = p.interactive;
    wordcraft_ui_egui::Services {
        clipboard_text: Some(Box::new(move || c1.text())),
        clipboard_html: Some(Box::new(move || c2.html())),
        clipboard_rtf: Some(Box::new(move || c3.rtf())),
        clipboard_formats: Some(Box::new(move || c4.formats())),
        clipboard_is_own: Some(Box::new(move || o1.is_own(c5.as_ref()))),
        set_clipboard_rich: Some(Box::new(move |text, html, rtf, mathml| {
            o2.set(c6.as_ref(), &Copy { text: text.into(), html: html.into(), rtf: rtf.into(), mathml: mathml.into(), png: Vec::new() })
        })),
        set_clipboard_image: Some(Box::new({
            let cb = c7.clone();
            move |img, html, text| {
                let Some(png) = png_from_rgba(img.width, img.height, &img.rgba) else { return false };
                o3.set(cb.as_ref(), &Copy { text: text.into(), html: html.into(), png, ..Default::default() })
            }
        })),
        clipboard_picture: Some(Box::new(move || {
            let png = c7.picture_png()?;
            Some(match rgba_from_png(&png) {
                Some(img) => ClipboardPicture::Pixels { width: img.width() as usize, height: img.height() as usize, rgba: img.into_raw() },
                None => ClipboardPicture::Encoded(png),
            })
        })),
        pick_open: if interactive {
            Some(Box::new(|what: &str| match what {
                "picture" => files::pick_open("Insert Picture", "Pictures", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"]),
                _ => files::pick_open("Open", "Documents", &["docx", "doc", "odt", "rtf", "html", "htm", "md", "markdown", "tex", "txt"]),
            }) as Box<dyn Fn(&str) -> Option<String>>)
        } else {
            None
        },
        pick_save: if interactive { Some(Box::new(files::pick_save) as Box<dyn Fn(&str) -> Option<String>>) } else { None },
        printers: if interactive { Some(Box::new(print::discover)) } else { None },
        print_native: if interactive {
            Some(Box::new(|pdf, settings, collate, options| print::submit(pdf, settings, collate, options, "OfficeCraft document"))
                as wordcraft_ui_egui::printing::Submit)
        } else {
            None
        },
        ..Default::default()
    }
}

/// GridCraft's services (`gridcraft_ui_egui::Services`).
pub fn sheet_services(p: &Platform) -> gridcraft_ui_egui::Services {
    let cb = p.clipboard.clone();
    let (c1, c2, c3, c4) = (cb.clone(), cb.clone(), cb.clone(), cb);
    let interactive = p.interactive;
    gridcraft_ui_egui::Services {
        copy_html: Some(Box::new(move |html: &str, text: &str| {
            if c1.set(&Copy { text: text.into(), html: html.into(), ..Default::default() }).is_some() {
                Ok(())
            } else {
                Err("the clipboard refused the copy".into())
            }
        })),
        paste_html: Some(Box::new(move || c2.html())),
        paste_text: Some(Box::new(move || c3.text())),
        paste_image: Some(Box::new(move || c4.picture_png().and_then(|png| rgba_from_png(&png)))),
        pick_open: if interactive {
            Some(Box::new(|| files::pick_open("Open", "Spreadsheets", &["xlsx", "csv", "tsv"])) as Box<dyn Fn() -> Option<String>>)
        } else {
            None
        },
        pick_save: if interactive { Some(Box::new(|s: &str| files::pick_save(s))) } else { None },
        pick_picture: if interactive {
            Some(Box::new(|| files::pick_open("Insert Picture", "Pictures", &["png", "jpg", "jpeg"])) as Box<dyn Fn() -> Option<String>>)
        } else {
            None
        },
        open_url: Some(Box::new(|url: &str| files::open_url(url))),
        printers: if interactive { Some(Box::new(print::discover)) } else { None },
        print_native: if interactive {
            Some(Box::new(|pdf, settings| print::submit(pdf, settings, true, Vec::new(), "OfficeCraft workbook"))
                as gridcraft_ui_egui::printing::Submit)
        } else {
            None
        },
        ..Default::default()
    }
}

/// DeckCraft's services (`deckcraft_ui_egui::Services`). Its Print exports a PDF and opens it;
/// the window opens that PDF in a tab of its own (see the app).
pub fn deck_services(p: &Platform) -> deckcraft_ui_egui::Services {
    let cb = p.clipboard.clone();
    let (c1, c2, c3) = (cb.clone(), cb.clone(), cb);
    let interactive = p.interactive;
    deckcraft_ui_egui::Services {
        clipboard_html: Some(Box::new(move || c1.html())),
        clipboard_set_html: Some(Box::new(move |html: &str, text: &str| {
            c2.set(&Copy { text: text.into(), html: html.into(), ..Default::default() }).is_some()
        })),
        clipboard_image: Some(Box::new(move || c3.picture_png())),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        pick_open: if interactive {
            Some(Box::new(|what: &str| match what {
                "picture" => files::pick_open("Insert Picture", "Pictures", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"]),
                _ => files::pick_open("Open", "Presentations", &["pptx"]),
            }))
        } else {
            None
        },
        // Always set: DeckCraft takes a missing save panel to mean it runs in a browser (its
        // Print would then try to download). Isolated, the panel answers "cancelled".
        pick_save: Some(Box::new(move |s: &str| if interactive { files::pick_save(s) } else { None })),
        ..Default::default()
    }
}
