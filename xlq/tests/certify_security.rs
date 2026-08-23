//! SECURITY-parts mutation matrix — certify must REFUSE any tamper with a security-relevant
//! opaque part (external data-source target, protection control, ribbon autorun) and must fail
//! closed on any unknown reference-bearing part. These are the changes the cell diff never sees;
//! certifying one would launder a security regression (SSRF, macro autorun, stripped protection).

mod common;
use common::*;

fn edit() -> Edit {
    Edit::insert_rows("Sheet1", 5, 1)
}

#[test]
fn repointed_data_connection_is_refused() {
    // A foreign edit that repoints the workbook's external data source to an attacker host.
    assert_mangle_refused(
        "security.xlsx",
        &edit(),
        mangle::repoint_connection,
        "repoint_connection",
    );
}

#[test]
fn stripped_protection_is_refused() {
    assert_mangle_refused(
        "security.xlsx",
        &edit(),
        mangle::strip_sheet_protection,
        "strip_sheet_protection",
    );
    assert_mangle_refused(
        "security.xlsx",
        &edit(),
        mangle::strip_workbook_protection,
        "strip_workbook_protection",
    );
}

#[test]
fn injected_customui_autorun_is_refused() {
    assert_mangle_refused(
        "security.xlsx",
        &edit(),
        mangle::inject_customui_onload,
        "inject_customui_onload",
    );
}

#[test]
fn unknown_reference_part_fails_closed() {
    // A part outside certify's verified/known-safe surface must be refused, not silently accepted.
    assert_mangle_refused(
        "security.xlsx",
        &edit(),
        mangle::inject_unknown_part,
        "inject_unknown_part",
    );
}
