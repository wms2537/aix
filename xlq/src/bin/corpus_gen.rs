//! Generates the reference-completeness TEST CORPUS into tests/fixtures/corpus/.
//!
//! CONTRACT: `cargo run --features devtools --bin corpus-gen -- tests/fixtures/corpus`
//! writes a set of small, hand-authored .xlsx workbooks, each carrying POPULATED `<v>`
//! formula caches.
//!
//! WHY HAND-AUTHORED (not built through ironcalc like `xlq-fixtures`): ironcalc's writer
//! BLANKS every formula cell's `<v>` on `save_to_xlsx`, so a workbook produced through the
//! engine can never carry a stale cache. Every fixture in the suite shipped a blank `<v/>`
//! cache for 31 hardening rounds, which hid a HIGH silent-wrong bug (restructure copied a
//! stale cache verbatim). This corpus exists to close that blind spot: the cache-soundness
//! property (src/tests_cache_soundness.rs) runs over these POPULATED caches, so a
//! reintroduced stale-cache defect fails a test instead of reaching production.
//!
//! Deterministic: fixed zip mtimes, no RNG — regenerating yields byte-identical files.
//! The corpus is committed (Cargo.toml `include = ["tests/fixtures/**"]`), so a plain
//! `cargo test` consumes it without needing this generator.

// This generator grows fixture-by-fixture; builder methods for parts not yet emitted (extras,
// extra content-type Defaults) are intentionally retained for the next fixtures.
#![allow(dead_code)]

use std::io::Write;
use std::path::Path;

/// An ordered list of (zip-part-name, bytes).
type Parts = Vec<(String, Vec<u8>)>;

const SS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// A sheet: display name + the FULL `<worksheet>…</worksheet>` XML.
struct Sheet {
    name: &'static str,
    xml: String,
}

/// An extra (non-standard) part: zip path, its `[Content_Types]` Override content-type
/// (empty = rely on a Default extension), and bytes.
struct Extra {
    path: String,
    content_type: &'static str,
    data: String,
}

struct Book {
    sheets: Vec<Sheet>,
    defined_names: String,
    calc_pr: String,
    /// Injected right after `<workbook …>` (e.g. `<workbookProtection …/>`).
    wb_extra: String,
    /// Extra workbook relationships as (id, type-suffix, target).
    wb_rels: Vec<(String, String, String)>,
    extras: Vec<Extra>,
    /// Extra `[Content_Types]` Default entries as (extension, content-type).
    defaults: Vec<(&'static str, &'static str)>,
}

impl Book {
    fn new() -> Self {
        Book {
            sheets: Vec::new(),
            defined_names: String::new(),
            calc_pr: r#"<calcPr calcId="124519"/>"#.into(),
            wb_extra: String::new(),
            wb_rels: Vec::new(),
            extras: Vec::new(),
            defaults: Vec::new(),
        }
    }
    fn sheet(mut self, name: &'static str, xml: String) -> Self {
        self.sheets.push(Sheet { name, xml });
        self
    }
    fn names(mut self, dn: &str) -> Self {
        self.defined_names = dn.into();
        self
    }
    fn calc(mut self, c: &str) -> Self {
        self.calc_pr = c.into();
        self
    }
    fn wb_extra(mut self, s: &str) -> Self {
        self.wb_extra = s.into();
        self
    }
    fn wb_rel(mut self, id: &str, type_suffix: &str, target: &str) -> Self {
        self.wb_rels
            .push((id.into(), type_suffix.into(), target.into()));
        self
    }
    fn extra(mut self, path: &str, content_type: &'static str, data: String) -> Self {
        self.extras.push(Extra {
            path: path.into(),
            content_type,
            data,
        });
        self
    }
    fn default_ct(mut self, ext: &'static str, ct: &'static str) -> Self {
        self.defaults.push((ext, ct));
        self
    }

    /// Assemble the workbook as an ordered list of (part-name, bytes).
    fn parts(&self) -> Parts {
        let mut p: Parts = Vec::new();

        // [Content_Types].xml
        let mut ct = String::new();
        ct.push_str(&format!(r#"<Types xmlns="{CT}">"#));
        ct.push_str(r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#);
        ct.push_str(r#"<Default Extension="xml" ContentType="application/xml"/>"#);
        for (ext, c) in &self.defaults {
            ct.push_str(&format!(
                r#"<Default Extension="{ext}" ContentType="{c}"/>"#
            ));
        }
        ct.push_str(r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#);
        ct.push_str(r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#);
        for i in 0..self.sheets.len() {
            ct.push_str(&format!(
                r#"<Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
                i + 1
            ));
        }
        for e in &self.extras {
            if !e.content_type.is_empty() {
                ct.push_str(&format!(
                    r#"<Override PartName="/{}" ContentType="{}"/>"#,
                    e.path, e.content_type
                ));
            }
        }
        ct.push_str("</Types>");
        p.push(("[Content_Types].xml".into(), ct.into_bytes()));

        // _rels/.rels
        p.push((
            "_rels/.rels".into(),
            format!(
                r#"<Relationships xmlns="{PKG}"><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            )
            .into_bytes(),
        ));

        // xl/workbook.xml
        let mut sheets_xml = String::new();
        for (i, s) in self.sheets.iter().enumerate() {
            sheets_xml.push_str(&format!(
                r#"<sheet name="{}" sheetId="{}" r:id="rIdS{}"/>"#,
                s.name,
                i + 1,
                i + 1
            ));
        }
        let dn = if self.defined_names.is_empty() {
            String::new()
        } else {
            format!("<definedNames>{}</definedNames>", self.defined_names)
        };
        p.push((
            "xl/workbook.xml".into(),
            format!(
                r#"<workbook xmlns="{SS}" xmlns:r="{REL}">{}<sheets>{sheets_xml}</sheets>{dn}{}</workbook>"#,
                self.wb_extra, self.calc_pr
            )
            .into_bytes(),
        ));

        // xl/_rels/workbook.xml.rels
        let mut wr = String::new();
        wr.push_str(&format!(r#"<Relationships xmlns="{PKG}">"#));
        for i in 0..self.sheets.len() {
            wr.push_str(&format!(
                r#"<Relationship Id="rIdS{}" Type="{REL}/worksheet" Target="worksheets/sheet{}.xml"/>"#,
                i + 1,
                i + 1
            ));
        }
        wr.push_str(&format!(
            r#"<Relationship Id="rIdStyles" Type="{REL}/styles" Target="styles.xml"/>"#
        ));
        for (id, ty, target) in &self.wb_rels {
            wr.push_str(&format!(
                r#"<Relationship Id="{id}" Type="{REL}/{ty}" Target="{target}"/>"#
            ));
        }
        wr.push_str("</Relationships>");
        p.push(("xl/_rels/workbook.xml.rels".into(), wr.into_bytes()));

        // xl/styles.xml — cellXfs: index 0 = General, index 1 = builtin numFmt 1 ("0",
        // integer display) used by the precision-as-displayed fixture.
        p.push((
            "xl/styles.xml".into(),
            format!(
                r#"<styleSheet xmlns="{SS}"><numFmts count="0"/><fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="1" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/></styleSheet>"#
            )
            .into_bytes(),
        ));

        // sheets
        for (i, s) in self.sheets.iter().enumerate() {
            p.push((
                format!("xl/worksheets/sheet{}.xml", i + 1),
                s.xml.clone().into_bytes(),
            ));
        }

        // extras
        for e in &self.extras {
            p.push((e.path.clone(), e.data.clone().into_bytes()));
        }
        p
    }
}

/// A worksheet from its `<sheetData>` inner rows, with a dimension.
fn ws(dim: &str, rows: &str) -> String {
    ws_full(dim, rows, "")
}

/// A worksheet with a `trailer` appended after `</sheetData>` (sheetProtection, mergeCells,
/// dataValidations, hyperlinks, … — in OOXML schema order).
fn ws_full(dim: &str, rows: &str, trailer: &str) -> String {
    format!(
        r#"<worksheet xmlns="{SS}" xmlns:r="{REL}"><dimension ref="{dim}"/><sheetData>{rows}</sheetData>{trailer}</worksheet>"#
    )
}

/// A literal number cell `<c r=..><v>..</v></c>`.
fn num(r: &str, v: &str) -> String {
    format!(r#"<c r="{r}"><v>{v}</v></c>"#)
}

/// A formula cell WITH A POPULATED cache: `<c r=..><f>..</f><v>..</v></c>`.
fn fcell(r: &str, f: &str, v: &str) -> String {
    format!(r#"<c r="{r}"><f>{f}</f><v>{v}</v></c>"#)
}

fn write_zip(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (name, data) in parts {
        z.start_file(name.as_str(), opts).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

// ---- the corpus workbooks (each carries POPULATED caches) --------------------------------

/// sum_band: the flagship cache fixture. A `SUM` over a data band, an absolute ref, a
/// straddle range, all with CORRECT populated caches — so a stale-cache regression (or a
/// value mis-shift) makes the recomputed value diverge from the stored one.
fn sum_band() -> Vec<u8> {
    let mut rows = String::new();
    rows.push_str(r#"<row r="1">"#);
    rows.push_str(&num("A1", "1"));
    rows.push_str(&fcell("B1", "SUM(A1:A10)", "55"));
    rows.push_str(&fcell("C1", "$A$8", "8"));
    rows.push_str(&fcell("D1", "SUM(A4:A6)", "15"));
    rows.push_str("</row>");
    for r in 2..=10 {
        rows.push_str(&format!(
            r#"<row r="{r}">{}</row>"#,
            num(&format!("A{r}"), &r.to_string())
        ));
    }
    rows.push_str(&format!(
        r#"<row r="13">{}</row>"#,
        fcell("A13", "A5*2", "10")
    ));
    let book = Book::new().sheet("Sheet1", ws("A1:D13", &rows));
    write_zip(&book.parts())
}

/// crosssheet: cross-sheet references with populated caches — a value on Sheet2 depends on a
/// data band on Sheet1, so editing Sheet1 changes Sheet2's cache (transitive staleness).
fn crosssheet() -> Vec<u8> {
    let mut s1 = String::new();
    for r in 1..=10 {
        s1.push_str(&format!(
            r#"<row r="{r}">{}</row>"#,
            num(&format!("A{r}"), &r.to_string())
        ));
    }
    let s2 = format!(
        r#"<row r="1">{}{}</row>"#,
        fcell("B1", "Sheet1!A5", "5"),
        fcell("C1", "SUM(Sheet1!A1:A6)", "21")
    );
    let s3 = format!(r#"<row r="1">{}</row>"#, num("A1", "100"));
    let book = Book::new()
        .sheet("Sheet1", ws("A1:A10", &s1))
        .sheet("Sheet2", ws("B1:C1", &s2))
        .sheet("Sheet3", ws("A1:A1", &s3));
    write_zip(&book.parts())
}

/// settings: precision-as-displayed (`fullPrecision="0"`) with a downstream formula whose
/// full-precision result (1.4) differs from its displayed one (1), plus a normal cache. This
/// is the fixture that would have caught the round-33 oracle/fullPrecision false-certify.
fn settings() -> Vec<u8> {
    // A1 = 7/5 (=1.4) formatted "0" (style s=1 -> displays 1); B1 = A1; caches at full precision.
    let rows = format!(
        r#"<row r="1"><c r="A1" s="1"><f>7/5</f><v>1.4</v></c>{}</row>"#,
        fcell("B1", "A1", "1.4")
    );
    let book = Book::new()
        .sheet("Sheet1", ws("A1:B1", &rows))
        .calc(r#"<calcPr calcId="124519" fullPrecision="0"/>"#);
    write_zip(&book.parts())
}

/// names: a global defined name whose refers-to targets a cell BELOW a typical insert point,
/// so the name (and a formula reading it) must both shift — exercising the defined-name
/// comparator and the no-A1-token-survives property. Caches are ironcalc-faithful (the
/// exotic non-ASCII / cell-shaped σ-name traps live in the refshift unit tests, where
/// ironcalc's inability to resolve them does not pollute the cache baseline).
fn names() -> Vec<u8> {
    let mut rows = String::new();
    rows.push_str(&format!(
        r#"<row r="1">{}{}</row>"#,
        num("A1", "1"),
        fcell("B1", "Total", "55")
    ));
    for r in 2..=10 {
        rows.push_str(&format!(
            r#"<row r="{r}">{}</row>"#,
            num(&format!("A{r}"), &r.to_string())
        ));
    }
    rows.push_str(&format!(
        r#"<row r="11">{}</row>"#,
        fcell("A11", "SUM(A1:A10)", "55")
    ));
    let dn = r#"<definedName name="Total">Sheet1!$A$11</definedName>"#;
    let book = Book::new().sheet("Sheet1", ws("A1:B11", &rows)).names(dn);
    write_zip(&book.parts())
}

/// security: the SECURITY-relevant opaque parts — an external data connection (webPr url),
/// sheet + workbook protection, and a customUI ribbon — plus a populated-cache formula. Drives
/// the security mangle matrix (repoint the URL, strip protection, inject an onLoad autorun, add
/// an unknown reference-bearing part) → certify must REFUSE each.
fn security() -> Vec<u8> {
    let rows = format!(
        r#"<row r="1">{}{}</row><row r="2">{}</row><row r="3">{}</row>"#,
        num("A1", "1"),
        fcell("B1", "SUM(A1:A3)", "6"),
        num("A2", "2"),
        num("A3", "3"),
    );
    let sheet = ws_full(
        "A1:B3",
        &rows,
        r#"<sheetProtection sheet="1" password="CC1A" objects="1" scenarios="1"/>"#,
    );
    let conn = format!(
        r#"<connections xmlns="{SS}"><connection id="1" name="q" type="4"><webPr url="https://data.internal.example.com/report.xml"/></connection></connections>"#
    );
    let customui = r#"<customUI xmlns="http://schemas.microsoft.com/office/2006/01/customui"><ribbon><tabs><tab id="t" label="T"/></tabs></ribbon></customUI>"#;
    let book = Book::new()
        .sheet("Sheet1", sheet)
        .wb_extra(r#"<workbookProtection workbookPassword="ABCD" lockStructure="1"/>"#)
        .wb_rel("rIdConn", "connections", "connections.xml")
        .extra(
            "xl/connections.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml",
            conn,
        )
        .extra(
            "customUI/customUI14.xml",
            "application/vnd.ms-office.customUI+xml",
            customui.into(),
        );
    write_zip(&book.parts())
}

/// constructs: coordinate-bearing NON-CELL constructs — a mergeCell, a dataValidation `sqref`, an
/// internal hyperlink, and a defined name — plus a populated cache. Drives the non-cell mangle
/// matrix (retarget the name, move the merge, retarget the hyperlink, flip date1904, set
/// fullPrecision) → certify must REFUSE each; a benign reserialize must still CERTIFY.
fn constructs() -> Vec<u8> {
    let mut rows = String::new();
    rows.push_str(&format!(
        r#"<row r="1">{}{}</row>"#,
        num("A1", "1"),
        fcell("B1", "Total", "55")
    ));
    for r in 2..=10 {
        rows.push_str(&format!(
            r#"<row r="{r}">{}</row>"#,
            num(&format!("A{r}"), &r.to_string())
        ));
    }
    rows.push_str(&format!(
        r#"<row r="11">{}</row>"#,
        fcell("A11", "SUM(A1:A10)", "55")
    ));
    let trailer = concat!(
        r#"<mergeCells count="1"><mergeCell ref="C1:D1"/></mergeCells>"#,
        r#"<dataValidations count="1"><dataValidation type="whole" sqref="A2"><formula1>0</formula1></dataValidation></dataValidations>"#,
        r#"<hyperlinks><hyperlink ref="A1" location="Sheet1!A11" display="jump"/></hyperlinks>"#,
    );
    let dn = r#"<definedName name="Total">Sheet1!$A$11</definedName>"#;
    let book = Book::new()
        .sheet("Sheet1", ws_full("A1:D11", &rows, trailer))
        .names(dn);
    write_zip(&book.parts())
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: corpus-gen <output_dir>");
        std::process::exit(2);
    });
    let dir = Path::new(&out);
    std::fs::create_dir_all(dir).expect("create corpus dir");
    type Fixture = (&'static str, fn() -> Vec<u8>);
    let fixtures: &[Fixture] = &[
        ("sum_band.xlsx", sum_band),
        ("crosssheet.xlsx", crosssheet),
        ("settings.xlsx", settings),
        ("names.xlsx", names),
        ("security.xlsx", security),
        ("constructs.xlsx", constructs),
    ];
    for (name, gen) in fixtures {
        let bytes = gen();
        let path = dir.join(name);
        std::fs::write(&path, &bytes).expect("write fixture");
        println!("{}  {} bytes", path.display(), bytes.len());
    }
}
