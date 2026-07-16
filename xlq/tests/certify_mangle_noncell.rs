//! NON-CELL reference mutation matrix — certify must REFUSE a divergence in a reference the
//! positional cell diff never compares: a defined name's target, a mergeCell/hyperlink extent, or
//! a value-affecting workbook setting. A benign reserialization of the same constructs must still
//! CERTIFY (the over-refusal guard).

mod common;
use common::*;

fn edit() -> Edit {
    Edit::insert_rows("Sheet1", 5, 1)
}

#[test]
fn retargeted_defined_name_is_refused() {
    assert_mangle_refused(
        "constructs.xlsx",
        &edit(),
        mangle::retarget_defined_name,
        "retarget_defined_name",
    );
}

#[test]
fn moved_mergecell_is_refused() {
    assert_mangle_refused(
        "constructs.xlsx",
        &edit(),
        mangle::move_mergecell,
        "move_mergecell",
    );
}

#[test]
fn retargeted_hyperlink_is_refused() {
    assert_mangle_refused(
        "constructs.xlsx",
        &edit(),
        mangle::retarget_hyperlink,
        "retarget_hyperlink",
    );
}

#[test]
fn flipped_date_epoch_is_refused() {
    // date1904 shifts every date value by 1462 days, invisible to a serial-vs-serial cell diff.
    assert_mangle_refused(
        "constructs.xlsx",
        &edit(),
        mangle::flip_date1904,
        "flip_date1904",
    );
}

#[test]
fn benign_reserialization_of_constructs_still_certifies() {
    assert_variant_certifies(
        "constructs.xlsx",
        &edit(),
        benign::reserialize_whitespace,
        "reserialize",
    );
}
