//! Benchmark helper: load a workbook into ironcalc and do NOTHING else.
//!
//! Exists so benchmarks/run_bench.sh can measure the engine's parse/load
//! time in isolation, instead of inferring it from `xlq inspect` (which
//! also hashes the file and computes the census).

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: load-only <file.xlsx>");
    ironcalc::import::load_from_xlsx(&path, "en", "UTC", "en").expect("load workbook");
}
