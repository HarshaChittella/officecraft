//! Printing through `craft-print` (CUPS on macOS), as the Craft apps do: printer discovery and
//! submission run on worker threads, never on the UI thread.

use craft_ui::printing::{Duplex, Printer, Settings};
use std::sync::mpsc::{Receiver, channel};

/// The printers, found on a worker.
pub fn discover() -> Receiver<Vec<Printer>> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("officecraft-print-discovery".into()).spawn(move || {
        let printers = craft_print::printers().into_iter().map(|p| Printer { name: p.name, label: p.description, default: p.default }).collect();
        let _ = tx.send(printers);
    });
    if let Err(e) = spawned {
        log::warn!("printer discovery: {e}");
    }
    rx
}

/// Sends `pdf` to the printer in `settings`, on a worker. `collate` and `options` are
/// WordCraft's extras (its PDF-page collation, spooler options); others pass `true` and none.
pub fn submit(pdf: Vec<u8>, settings: Settings, collate: bool, options: Vec<(String, String)>, title: &str) -> Receiver<Result<String, String>> {
    let (tx, rx) = channel();
    let job = craft_print::Job {
        printer: Some(settings.printer),
        copies: settings.copies.clamp(1, 999),
        collate,
        duplex: settings.duplex.map(|d| match d {
            Duplex::LongEdge => craft_print::Duplex::LongEdge,
            Duplex::ShortEdge => craft_print::Duplex::ShortEdge,
            Duplex::Off => craft_print::Duplex::Off,
        }),
        grayscale: settings.grayscale,
        scaling: craft_print::Scaling::ShrinkToFit,
        title: title.to_string(),
        options,
        ..Default::default()
    };
    let spawned = std::thread::Builder::new().name("officecraft-print-submit".into()).spawn(move || {
        let _ = tx.send(craft_print::submit(&pdf, &job).map_err(|e| e.to_string()));
    });
    if let Err(e) = spawned {
        log::warn!("print submission: {e}");
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Discovery answers (an empty list on a Mac with no printers) and a job to a printer that
    /// doesn't exist fails with a message: nothing hangs or panics.
    #[test]
    fn discovery_answers_and_a_missing_printer_is_an_error() {
        let printers = discover().recv_timeout(Duration::from_secs(20));
        assert!(printers.is_ok(), "discovery answered");
        let settings = Settings { printer: "officecraft-no-such-printer".into(), copies: 1, ..Default::default() };
        let result = submit(b"%PDF-1.4\n%%EOF\n".to_vec(), settings, true, Vec::new(), "test").recv_timeout(Duration::from_secs(20));
        assert!(matches!(result, Ok(Err(_))), "a missing printer is an error: {result:?}");
    }
}
