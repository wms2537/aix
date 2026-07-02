//! Minimal load→save roundtrip through ironcalc, for preservation benchmarks.
//! Usage: roundtrip <input.xlsx> <output.xlsx>
//! Loads the workbook (no evaluate) and immediately re-saves it, so the
//! output reflects exactly what ironcalc's reader+writer preserve.

use ironcalc::export::save_to_xlsx;
use ironcalc::import::load_from_xlsx;

fn main() {
    let mut args = std::env::args().skip(1);
    let (input, output) = match (args.next(), args.next()) {
        (Some(i), Some(o)) => (i, o),
        _ => {
            eprintln!("usage: roundtrip <input.xlsx> <output.xlsx>");
            std::process::exit(2);
        }
    };
    let model = match load_from_xlsx(&input, "en", "UTC", "en") {
        Ok(m) => m,
        Err(e) => {
            eprintln!("roundtrip: load {input}: {e:?}");
            std::process::exit(1);
        }
    };
    match std::fs::remove_file(&output) {
        Ok(()) | Err(_) => {}
    }
    if let Err(e) = save_to_xlsx(&model, &output) {
        eprintln!("roundtrip: save {output}: {e:?}");
        std::process::exit(1);
    }
}
