//! Generates the synthetic-twin fixture corpus into fixtures/.
//!
//! CONTRACT: `cargo run --bin xlq-fixtures -- <output_dir>` writes:
//!   1. branch-consolidation.xlsx — 5 branch P&L sheets + Consolidated sheet
//!      (cross-sheet SUMs building consolidated P&L and cash flow), ~14
//!      account rows x 12 months per branch. Planted defects: one #DIV/0!
//!      (margin on zero revenue), one SUM range that stops a row short, one
//!      hardcoded value pasted over a formula in the consolidated sheet.
//!   2. stock-reconciliation.xlsx — Movements sheet (600 rows: date, sku,
//!      branch, qty_in, qty_out), Sales sheet, Purchases sheet, Recon sheet
//!      using SUMIFS per sku x branch comparing movement totals to
//!      sales+purchases; planted mismatches and one #N/A from a lookup of a
//!      missing SKU.
//!   3. payroll.xlsx — Attendance sheet (40 employees x 31 days of hours),
//!      Rates sheet, Payroll sheet computing regular/overtime pay with
//!      IF/MAX/MIN and VLOOKUP into Rates; planted: one negative-hours typo
//!      and one employee missing from Rates (#N/A).
//!   4. claims.xlsx — Register of 300 claims (id, branch, category, amount,
//!      status, submitted, approved) with VLOOKUP category limits, an
//!      over-limit flag column, and status tally formulas; planted: one
//!      date-typo (approved before submitted) caught by a check column.
//!   5. perf-large.xlsx — performance fixture: one sheet, 200 cols x 500
//!      rows of SUM/AVERAGE/IF chains (~100k formula cells) for the
//!      benchmark harness.
//!
//! Built with ironcalc itself (Model::new_empty + set_user_input +
//! evaluate + save_to_xlsx) — no Python, no external deps. Deterministic:
//! any pseudo-random data uses a fixed-seed LCG, never a real RNG, so
//! fixture hashes are stable across runs.
//!
//! The planted-defect locations (1-based row/col) are printed to stderr as a
//! single JSON line so integration tests can verify detection.

use anyhow::{Context, Result};
use ironcalc::base::Model;
use ironcalc::export::save_to_xlsx;
use serde_json::{json, Value};
use std::io::{Read, Write};

// Pinned into workbook metadata and core.xml so fixture bytes are identical
// across runs; ironcalc otherwise stamps wall-clock times at save.
const FIXED_TIMESTAMP: &str = "2026-01-01T00:00:00Z";

struct Lcg(u64);

impl Lcg {
    fn new() -> Self {
        Lcg(42)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

fn col_name(col: i32) -> String {
    let mut col = col as u32;
    let mut bytes = Vec::new();
    while col > 0 {
        bytes.push(b'A' + ((col - 1) % 26) as u8);
        col = (col - 1) / 26;
    }
    bytes.reverse();
    String::from_utf8(bytes).expect("ascii")
}

fn new_model(first_sheet: &str, extra_sheets: &[&str]) -> Result<Model<'static>> {
    let mut model = Model::new_empty("fixture", "en", "UTC", "en").map_err(anyhow::Error::msg)?;
    model
        .rename_sheet("Sheet1", first_sheet)
        .map_err(anyhow::Error::msg)?;
    for name in extra_sheets {
        model.add_sheet(name).map_err(anyhow::Error::msg)?;
    }
    model.workbook.metadata.created = FIXED_TIMESTAMP.to_string();
    model.workbook.metadata.last_modified = FIXED_TIMESTAMP.to_string();
    Ok(model)
}

fn patch_modified_timestamp(core_xml: &str) -> String {
    let open = "<dcterms:modified xsi:type=\"dcterms:W3CDTF\">";
    let close = "</dcterms:modified>";
    match (core_xml.find(open), core_xml.find(close)) {
        (Some(start), Some(end)) if start + open.len() <= end => {
            format!(
                "{}{}{}",
                &core_xml[..start + open.len()],
                FIXED_TIMESTAMP,
                &core_xml[end..]
            )
        }
        _ => core_xml.to_string(),
    }
}

fn strip_zip_nondeterminism(path: &str) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {path}"))?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            out.add_directory(name, options)?;
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        if name == "docProps/core.xml" {
            data = patch_modified_timestamp(std::str::from_utf8(&data)?).into_bytes();
        }
        out.start_file(name, options)?;
        out.write_all(&data)?;
    }
    let cursor = out.finish()?;
    std::fs::write(path, cursor.into_inner()).with_context(|| format!("rewrite {path}"))
}

fn set(model: &mut Model, sheet: u32, row: i32, col: i32, value: impl Into<String>) -> Result<()> {
    let value = value.into();
    model
        .set_user_input(sheet, row, col, value.clone())
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("set sheet {sheet} R{row}C{col} = {value}"))
}

fn finish(model: &mut Model, dir: &str, name: &str) -> Result<()> {
    model.evaluate();
    let path = format!("{dir}/{name}");
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("remove existing {path}")),
    }
    save_to_xlsx(model, &path).map_err(|e| anyhow::anyhow!("save {path}: {e:?}"))?;
    strip_zip_nondeterminism(&path)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const ACCOUNTS: [&str; 15] = [
    "Revenue",
    "COGS",
    "Gross Profit",
    "Salaries",
    "Rent",
    "Utilities",
    "Marketing",
    "Insurance",
    "Depreciation",
    "Travel",
    "Supplies",
    "Maintenance",
    "Total Opex",
    "Operating Profit",
    "Margin",
];

fn build_branch_consolidation(dir: &str, defects: &mut Vec<Value>) -> Result<()> {
    let file = "branch-consolidation.xlsx";
    let branches = ["Branch1", "Branch2", "Branch3", "Branch4", "Branch5"];
    let mut m = new_model(
        "Branch1",
        &["Branch2", "Branch3", "Branch4", "Branch5", "Consolidated"],
    )?;
    let mut rng = Lcg::new();

    for si in 0..branches.len() as u32 {
        set(&mut m, si, 1, 1, "Account")?;
        for (mi, month) in MONTHS.iter().enumerate() {
            set(&mut m, si, 1, mi as i32 + 2, *month)?;
        }
        for (ai, account) in ACCOUNTS.iter().enumerate() {
            set(&mut m, si, ai as i32 + 2, 1, *account)?;
        }
        for c in 2..=13 {
            let cl = col_name(c);
            // Planted: Branch3 Jul has zero revenue, making the margin #DIV/0!.
            let revenue = if si == 2 && c == 8 {
                0
            } else {
                rng.range(80_000, 200_000)
            };
            let cogs = revenue * rng.range(40, 60) / 100;
            set(&mut m, si, 2, c, revenue.to_string())?;
            set(&mut m, si, 3, c, cogs.to_string())?;
            set(&mut m, si, 4, c, format!("={cl}2-{cl}3"))?;
            for r in 5..=13 {
                set(&mut m, si, r, c, rng.range(1_000, 12_000).to_string())?;
            }
            // Planted: Branch2 Feb Total Opex range stops one row short.
            let total = if si == 1 && c == 3 {
                format!("=SUM({cl}5:{cl}12)")
            } else {
                format!("=SUM({cl}5:{cl}13)")
            };
            set(&mut m, si, 14, c, total)?;
            set(&mut m, si, 15, c, format!("={cl}4-{cl}14"))?;
            set(&mut m, si, 16, c, format!("={cl}15/{cl}2"))?;
        }
    }

    let cons: u32 = 5;
    set(&mut m, cons, 1, 1, "Account")?;
    for (mi, month) in MONTHS.iter().enumerate() {
        set(&mut m, cons, 1, mi as i32 + 2, *month)?;
    }
    for (ai, account) in ACCOUNTS.iter().enumerate() {
        set(&mut m, cons, ai as i32 + 2, 1, *account)?;
    }
    let data_rows: [i32; 11] = [2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13];
    for c in 2..=13 {
        let cl = col_name(c);
        for r in data_rows {
            let args: Vec<String> = branches.iter().map(|b| format!("{b}!{cl}{r}")).collect();
            set(&mut m, cons, r, c, format!("=SUM({})", args.join(",")))?;
        }
        set(&mut m, cons, 4, c, format!("={cl}2-{cl}3"))?;
        set(&mut m, cons, 14, c, format!("=SUM({cl}5:{cl}13)"))?;
        set(&mut m, cons, 15, c, format!("={cl}4-{cl}14"))?;
        set(&mut m, cons, 16, c, format!("={cl}15/{cl}2"))?;
    }
    // Planted: constant pasted over the Salaries/Apr cross-sheet SUM.
    set(&mut m, cons, 5, 5, "54321")?;

    set(&mut m, cons, 18, 1, "Net Cash")?;
    set(&mut m, cons, 19, 1, "Cumulative Cash")?;
    for c in 2..=13 {
        let cl = col_name(c);
        set(&mut m, cons, 18, c, format!("={cl}15+{cl}10"))?;
        let cum = if c == 2 {
            "=B18".to_string()
        } else {
            format!("={}19+{cl}18", col_name(c - 1))
        };
        set(&mut m, cons, 19, c, cum)?;
    }

    finish(&mut m, dir, file)?;
    defects.push(json!({"file": file, "sheet": "Branch3", "row": 16, "col": 8, "kind": "div0_zero_revenue_margin"}));
    defects.push(
        json!({"file": file, "sheet": "Branch2", "row": 14, "col": 3, "kind": "sum_range_short"}),
    );
    defects.push(json!({"file": file, "sheet": "Consolidated", "row": 5, "col": 5, "kind": "hardcoded_over_formula"}));
    Ok(())
}

fn build_stock_reconciliation(dir: &str, defects: &mut Vec<Value>) -> Result<()> {
    let file = "stock-reconciliation.xlsx";
    let skus: Vec<String> = (1..=10).map(|i| format!("SKU-{i:03}")).collect();
    let branches = ["BR1", "BR2", "BR3"];
    let mut m = new_model("Movements", &["Sales", "Purchases", "Recon"])?;
    let mut rng = Lcg::new();

    struct Mv {
        date: String,
        sku: usize,
        branch: usize,
        qty_in: u64,
        qty_out: u64,
    }
    let mut movements: Vec<Mv> = Vec::with_capacity(600);
    for i in 0..598u64 {
        let date = format!("2026-{:02}-{:02}", 1 + (i / 100) % 6, 1 + i % 28);
        let sku = rng.range(0, 9) as usize;
        let branch = rng.range(0, 2) as usize;
        let qty = rng.range(1, 50);
        let inbound = rng.next().is_multiple_of(2);
        movements.push(Mv {
            date,
            sku,
            branch,
            qty_in: if inbound { qty } else { 0 },
            qty_out: if inbound { 0 } else { qty },
        });
    }
    // Fixed trailing movements anchor the two planted quantity mismatches.
    movements.push(Mv {
        date: "2026-06-27".into(),
        sku: 6,
        branch: 0,
        qty_in: 15,
        qty_out: 0,
    });
    movements.push(Mv {
        date: "2026-06-28".into(),
        sku: 2,
        branch: 1,
        qty_out: 20,
        qty_in: 0,
    });

    let mut sales: Vec<(String, usize, usize, u64)> = Vec::new();
    let mut purchases: Vec<(String, usize, usize, u64)> = Vec::new();
    for (i, mv) in movements.iter().enumerate() {
        if mv.qty_in > 0 {
            // Planted: the purchases record for the fixed SKU-007/BR1 movement is short by 7.
            let qty = if i == 598 { mv.qty_in - 7 } else { mv.qty_in };
            purchases.push((mv.date.clone(), mv.sku, mv.branch, qty));
        } else {
            // Planted: the sales record for the fixed SKU-003/BR2 movement is over by 5.
            let qty = if i == 599 { mv.qty_out + 5 } else { mv.qty_out };
            sales.push((mv.date.clone(), mv.sku, mv.branch, qty));
        }
    }

    for (h, header) in ["date", "sku", "branch", "qty_in", "qty_out"]
        .iter()
        .enumerate()
    {
        set(&mut m, 0, 1, h as i32 + 1, *header)?;
    }
    for (i, mv) in movements.iter().enumerate() {
        let r = i as i32 + 2;
        set(&mut m, 0, r, 1, mv.date.clone())?;
        set(&mut m, 0, r, 2, skus[mv.sku].clone())?;
        set(&mut m, 0, r, 3, branches[mv.branch])?;
        set(&mut m, 0, r, 4, mv.qty_in.to_string())?;
        set(&mut m, 0, r, 5, mv.qty_out.to_string())?;
    }
    for (sheet, rows) in [(1u32, &sales), (2u32, &purchases)] {
        for (h, header) in ["date", "sku", "branch", "qty"].iter().enumerate() {
            set(&mut m, sheet, 1, h as i32 + 1, *header)?;
        }
        for (i, (date, sku, branch, qty)) in rows.iter().enumerate() {
            let r = i as i32 + 2;
            set(&mut m, sheet, r, 1, date.clone())?;
            set(&mut m, sheet, r, 2, skus[*sku].clone())?;
            set(&mut m, sheet, r, 3, branches[*branch])?;
            set(&mut m, sheet, r, 4, qty.to_string())?;
        }
    }

    let recon: u32 = 3;
    let headers = [
        "sku",
        "branch",
        "mov_in",
        "mov_out",
        "purchases",
        "sales",
        "diff_in",
        "diff_out",
        "name",
    ];
    for (h, header) in headers.iter().enumerate() {
        set(&mut m, recon, 1, h as i32 + 1, *header)?;
    }
    set(&mut m, recon, 1, 11, "sku")?;
    set(&mut m, recon, 1, 12, "name")?;
    for (i, sku) in skus.iter().enumerate() {
        set(&mut m, recon, i as i32 + 2, 11, sku.clone())?;
        set(&mut m, recon, i as i32 + 2, 12, format!("Widget {}", i + 1))?;
    }
    let sales_last = sales.len() as i32 + 1;
    let purch_last = purchases.len() as i32 + 1;
    let write_recon_row = |m: &mut Model, r: i32| -> Result<()> {
        set(m, recon, r, 3, format!("=SUMIFS(Movements!$D$2:$D$601,Movements!$B$2:$B$601,$A{r},Movements!$C$2:$C$601,$B{r})"))?;
        set(m, recon, r, 4, format!("=SUMIFS(Movements!$E$2:$E$601,Movements!$B$2:$B$601,$A{r},Movements!$C$2:$C$601,$B{r})"))?;
        set(m, recon, r, 5, format!("=SUMIFS(Purchases!$D$2:$D${purch_last},Purchases!$B$2:$B${purch_last},$A{r},Purchases!$C$2:$C${purch_last},$B{r})"))?;
        set(m, recon, r, 6, format!("=SUMIFS(Sales!$D$2:$D${sales_last},Sales!$B$2:$B${sales_last},$A{r},Sales!$C$2:$C${sales_last},$B{r})"))?;
        set(m, recon, r, 7, format!("=C{r}-E{r}"))?;
        set(m, recon, r, 8, format!("=D{r}-F{r}"))?;
        set(
            m,
            recon,
            r,
            9,
            format!("=VLOOKUP($A{r},$K$2:$L$11,2,FALSE)"),
        )?;
        Ok(())
    };
    for (si, sku) in skus.iter().enumerate() {
        for (bi, branch) in branches.iter().enumerate() {
            let r = 2 + (si * 3 + bi) as i32;
            set(&mut m, recon, r, 1, sku.clone())?;
            set(&mut m, recon, r, 2, *branch)?;
            write_recon_row(&mut m, r)?;
        }
    }
    // Planted: SKU-999 exists nowhere, so its catalog VLOOKUP is #N/A.
    set(&mut m, recon, 32, 1, "SKU-999")?;
    set(&mut m, recon, 32, 2, "BR1")?;
    write_recon_row(&mut m, 32)?;

    finish(&mut m, dir, file)?;
    defects.push(json!({"file": file, "sheet": "Recon", "row": 20, "col": 7, "kind": "purchases_qty_mismatch"}));
    defects.push(
        json!({"file": file, "sheet": "Recon", "row": 9, "col": 8, "kind": "sales_qty_mismatch"}),
    );
    defects.push(
        json!({"file": file, "sheet": "Recon", "row": 32, "col": 9, "kind": "na_missing_sku"}),
    );
    Ok(())
}

fn build_payroll(dir: &str, defects: &mut Vec<Value>) -> Result<()> {
    let file = "payroll.xlsx";
    let mut m = new_model("Attendance", &["Rates", "Payroll"])?;
    let mut rng = Lcg::new();
    let ids: Vec<String> = (1..=40).map(|i| format!("E{i:03}")).collect();

    set(&mut m, 0, 1, 1, "emp_id")?;
    for d in 1..=31 {
        set(&mut m, 0, 1, d + 1, format!("D{d}"))?;
    }
    for (i, id) in ids.iter().enumerate() {
        let r = i as i32 + 2;
        set(&mut m, 0, r, 1, id.clone())?;
        for d in 1..=31 {
            // Planted: E013 day 17 has negative hours.
            let hours = if i == 12 && d == 17 {
                "-8".to_string()
            } else {
                rng.range(0, 10).to_string()
            };
            set(&mut m, 0, r, d + 1, hours)?;
        }
    }

    set(&mut m, 1, 1, 1, "emp_id")?;
    set(&mut m, 1, 1, 2, "rate")?;
    let mut r = 2;
    for (i, id) in ids.iter().enumerate() {
        // Planted: E027 is missing from Rates.
        if i == 26 {
            continue;
        }
        set(&mut m, 1, r, 1, id.clone())?;
        set(&mut m, 1, r, 2, rng.range(15, 45).to_string())?;
        r += 1;
    }

    let headers = [
        "emp_id",
        "total_hours",
        "regular_hours",
        "overtime_hours",
        "rate",
        "regular_pay",
        "overtime_pay",
        "gross_pay",
    ];
    for (h, header) in headers.iter().enumerate() {
        set(&mut m, 2, 1, h as i32 + 1, *header)?;
    }
    for (i, id) in ids.iter().enumerate() {
        let r = i as i32 + 2;
        set(&mut m, 2, r, 1, id.clone())?;
        set(&mut m, 2, r, 2, format!("=SUM(Attendance!B{r}:AF{r})"))?;
        set(&mut m, 2, r, 3, format!("=MIN(B{r},160)"))?;
        set(&mut m, 2, r, 4, format!("=MAX(B{r}-160,0)"))?;
        set(
            &mut m,
            2,
            r,
            5,
            format!("=VLOOKUP(A{r},Rates!$A$2:$B$40,2,FALSE)"),
        )?;
        set(&mut m, 2, r, 6, format!("=C{r}*E{r}"))?;
        set(&mut m, 2, r, 7, format!("=IF(D{r}>0,D{r}*E{r}*1.5,0)"))?;
        set(&mut m, 2, r, 8, format!("=F{r}+G{r}"))?;
    }

    finish(&mut m, dir, file)?;
    defects.push(json!({"file": file, "sheet": "Attendance", "row": 14, "col": 18, "kind": "negative_hours"}));
    defects.push(
        json!({"file": file, "sheet": "Payroll", "row": 28, "col": 5, "kind": "na_missing_rate"}),
    );
    Ok(())
}

fn build_claims(dir: &str, defects: &mut Vec<Value>) -> Result<()> {
    let file = "claims.xlsx";
    let mut m = new_model("Claims", &["Limits"])?;
    let mut rng = Lcg::new();
    let categories = ["Auto", "Property", "Medical", "Liability", "Travel"];
    let limits = [5_000, 20_000, 10_000, 15_000, 3_000];
    let statuses = ["Submitted", "Approved", "Rejected", "Paid"];
    let branches = ["BR1", "BR2", "BR3", "BR4", "BR5"];

    set(&mut m, 1, 1, 1, "category")?;
    set(&mut m, 1, 1, 2, "limit")?;
    for (i, (cat, lim)) in categories.iter().zip(limits.iter()).enumerate() {
        set(&mut m, 1, i as i32 + 2, 1, *cat)?;
        set(&mut m, 1, i as i32 + 2, 2, lim.to_string())?;
    }

    let headers = [
        "id",
        "branch",
        "category",
        "amount",
        "status",
        "submitted",
        "approved",
        "limit",
        "over_limit",
        "date_check",
    ];
    for (h, header) in headers.iter().enumerate() {
        set(&mut m, 0, 1, h as i32 + 1, *header)?;
    }
    for i in 0..300 {
        let r = i + 2;
        set(&mut m, 0, r, 1, format!("CLM-{:04}", i + 1))?;
        set(&mut m, 0, r, 2, branches[rng.range(0, 4) as usize])?;
        set(&mut m, 0, r, 3, categories[rng.range(0, 4) as usize])?;
        set(&mut m, 0, r, 4, rng.range(100, 25_000).to_string())?;
        let month = rng.range(1, 6);
        let day = rng.range(1, 28);
        // Planted: claim on row 138 was "approved" before it was submitted.
        if i == 136 {
            set(&mut m, 0, r, 5, "Approved")?;
            set(&mut m, 0, r, 6, "=DATE(2026,4,20)")?;
            set(&mut m, 0, r, 7, "=DATE(2026,4,8)")?;
        } else {
            let status = statuses[rng.range(0, 3) as usize];
            set(&mut m, 0, r, 5, status)?;
            set(&mut m, 0, r, 6, format!("=DATE(2026,{month},{day})"))?;
            if status == "Approved" || status == "Paid" {
                let delta = rng.range(1, 30);
                // Excel DATE normalizes day overflow into the next month.
                set(
                    &mut m,
                    0,
                    r,
                    7,
                    format!("=DATE(2026,{month},{})", day + delta),
                )?;
            }
        }
        set(
            &mut m,
            0,
            r,
            8,
            format!("=VLOOKUP($C{r},Limits!$A$2:$B$6,2,FALSE)"),
        )?;
        set(&mut m, 0, r, 9, format!("=IF(D{r}>H{r},\"OVER\",\"OK\")"))?;
        set(
            &mut m,
            0,
            r,
            10,
            format!("=IF(G{r}=\"\",\"OK\",IF(G{r}<F{r},\"BAD_DATE\",\"OK\"))"),
        )?;
    }
    set(&mut m, 0, 1, 12, "status")?;
    set(&mut m, 0, 1, 13, "count")?;
    for (i, status) in statuses.iter().enumerate() {
        let r = i as i32 + 2;
        set(&mut m, 0, r, 12, *status)?;
        set(&mut m, 0, r, 13, format!("=COUNTIF($E$2:$E$301,L{r})"))?;
    }

    finish(&mut m, dir, file)?;
    defects.push(json!({"file": file, "sheet": "Claims", "row": 138, "col": 7, "kind": "approved_before_submitted"}));
    Ok(())
}

fn build_perf_large(dir: &str) -> Result<()> {
    let file = "perf-large.xlsx";
    let mut m = new_model("Perf", &[])?;
    let mut rng = Lcg::new();
    for c in 1..=200 {
        set(&mut m, 0, 1, c, rng.range(1, 1000).to_string())?;
    }
    for r in 2..=500 {
        for c in 1..=200 {
            let cl = col_name(c);
            let p = r - 1;
            let end = col_name((c + 3).min(200));
            let formula = match (r + c) % 4 {
                0 => format!("={cl}{p}+1"),
                1 => format!("=SUM({cl}{p}:{end}{p})"),
                2 => format!("=AVERAGE({cl}{p}:{end}{p})"),
                _ => format!("=IF({cl}{p}>500,{cl}{p}-500,{cl}{p}+1)"),
            };
            set(&mut m, 0, r, c, formula)?;
        }
    }
    finish(&mut m, dir, file)
}

fn run() -> Result<()> {
    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fixtures".to_string());
    std::fs::create_dir_all(&out_dir).with_context(|| format!("create {out_dir}"))?;
    let mut defects: Vec<Value> = Vec::new();
    build_branch_consolidation(&out_dir, &mut defects)?;
    build_stock_reconciliation(&out_dir, &mut defects)?;
    build_payroll(&out_dir, &mut defects)?;
    build_claims(&out_dir, &mut defects)?;
    build_perf_large(&out_dir)?;
    eprintln!("{}", json!({ "defects": defects }));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ironcalc::base::types::CellType;

    #[test]
    fn lcg_is_deterministic() {
        let mut a = Lcg::new();
        let mut b = Lcg::new();
        for _ in 0..100 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn col_names() {
        assert_eq!(col_name(1), "A");
        assert_eq!(col_name(26), "Z");
        assert_eq!(col_name(27), "AA");
        assert_eq!(col_name(32), "AF");
        assert_eq!(col_name(200), "GR");
    }

    #[test]
    fn patch_modified_timestamp_replaces_only_modified() {
        let xml = "<dcterms:created xsi:type=\"dcterms:W3CDTF\">2020-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2020-06-07T08:09:10Z</dcterms:modified>";
        let patched = patch_modified_timestamp(xml);
        assert!(patched.contains("2020-01-02T03:04:05Z"));
        assert!(patched.contains(&format!(
            "<dcterms:modified xsi:type=\"dcterms:W3CDTF\">{FIXED_TIMESTAMP}</dcterms:modified>"
        )));
        assert!(!patched.contains("2020-06-07T08:09:10Z"));
        assert_eq!(patch_modified_timestamp("<x/>"), "<x/>");
    }

    #[test]
    fn empty_string_comparison_matches_excel() {
        let mut m = new_model("S", &[]).unwrap();
        set(&mut m, 0, 1, 2, "=IF(A1=\"\",\"empty\",\"full\")").unwrap();
        m.evaluate();
        assert_eq!(m.get_formatted_cell_value(0, 1, 2).unwrap(), "empty");
    }

    #[test]
    fn planted_defects_evaluate_as_expected() {
        let dir = std::env::temp_dir().join(format!("xlq-fixtures-test-{}", std::process::id()));
        let dir = dir.to_str().unwrap().to_string();
        std::fs::create_dir_all(&dir).unwrap();

        let mut m = new_model("A", &["B"]).unwrap();
        set(&mut m, 0, 1, 1, "10").unwrap();
        set(&mut m, 0, 2, 1, "0").unwrap();
        set(&mut m, 0, 3, 1, "=A1/A2").unwrap();
        set(&mut m, 1, 1, 1, "=SUM(A!A1,A!A2)").unwrap();
        m.evaluate();
        assert!(matches!(
            m.get_cell_type(0, 3, 1).unwrap(),
            CellType::ErrorValue
        ));
        assert_eq!(m.get_formatted_cell_value(1, 1, 1).unwrap(), "10");
        std::fs::remove_dir_all(&dir).ok();
    }
}
