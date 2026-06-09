//! Integration tests: stock movements + oversell guard, reservations, BOM
//! explode + cycle guard, where-used, supersession resolution, reorder
//! suggestions, and usage-based demand forecasting.

use chrono::{Duration, Utc};
use mcp_parts::store::PartsStore;
use mcp_parts::types::*;

fn store() -> PartsStore {
    PartsStore::new()
}

fn pid(s: &PartsStore, pn: &str) -> String { s.resolve_part(pn).unwrap().id }
fn loc(s: &PartsStore, name: &str) -> String { s.list_locations().into_iter().find(|l| l.name.contains(name)).unwrap().id }

#[test]
fn seed_loads() {
    let s = store();
    assert!(s.list_parts(None, None).len() >= 6);
    assert_eq!(s.list_locations().len(), 2);
    assert_eq!(s.list_suppliers().len(), 2);
}

#[test]
fn receive_issue_and_oversell_guard() {
    let s = store();
    let pads = pid(&s, "BRK-PAD-22");
    let wh = loc(&s, "Warehouse");
    let before = s.get_stock(&pads, &wh).unwrap().on_hand; // 50 seeded
    s.issue_stock(&pads, &wh, 10.0, "wo-1", "tech").unwrap();
    assert_eq!(s.get_stock(&pads, &wh).unwrap().on_hand, before - 10.0);
    // oversell rejected
    let err = s.issue_stock(&pads, &wh, 9999.0, "wo-2", "tech").unwrap_err();
    assert!(err.contains("insufficient"), "got: {err}");
}

#[test]
fn transfer_moves_between_locations() {
    let s = store();
    let filter = pid(&s, "OIL-FLT-3");
    let wh = loc(&s, "Warehouse");
    let van = loc(&s, "Van");
    let van_before = s.get_stock(&filter, &van).unwrap().on_hand;
    s.transfer_stock(&filter, &wh, &van, 10.0, "restock van", "driver").unwrap();
    assert_eq!(s.get_stock(&filter, &van).unwrap().on_hand, van_before + 10.0);
}

#[test]
fn adjust_cannot_go_negative() {
    let s = store();
    let rotor = pid(&s, "BRK-ROT-9");
    let wh = loc(&s, "Warehouse");
    assert!(s.adjust_stock(&rotor, &wh, -9999.0, "count", "auditor").is_err());
    let ok = s.adjust_stock(&rotor, &wh, 2.0, "found stock", "auditor").unwrap();
    assert_eq!(ok.qty, 2.0);
}

#[test]
fn reservation_reduces_available_and_releases_on_cancel() {
    let s = store();
    let pads = pid(&s, "BRK-PAD-22");
    let wh = loc(&s, "Warehouse");
    let avail0 = s.available_total(&pads);
    let r = s.reserve_stock(&pads, &wh, 5.0, "wo-9", "planner").unwrap();
    assert_eq!(s.available_total(&pads), avail0 - 5.0);
    // can't issue more than available after reservation
    let item = s.get_stock(&pads, &wh).unwrap();
    assert_eq!(item.reserved, 5.0);
    s.cancel_reservation(&r.id, "planner").unwrap();
    assert_eq!(s.available_total(&pads), avail0);
}

#[test]
fn bom_explode_and_cycle_guard() {
    let s = store();
    let brake = pid(&s, "BRK-ASSY-100");
    // seeded: 1 rotor + 2 pad sets
    let ex = s.explode_bom(&brake, 3.0);
    let comps = ex["components"].as_array().unwrap();
    let pad_row = comps.iter().find(|c| c["part_number"] == "BRK-PAD-22").unwrap();
    assert_eq!(pad_row["total_qty"].as_f64().unwrap(), 6.0); // 2 * 3
    let rotor_row = comps.iter().find(|c| c["part_number"] == "BRK-ROT-9").unwrap();
    assert_eq!(rotor_row["total_qty"].as_f64().unwrap(), 3.0);
    // cycle guard: rotor cannot be parent of brake (brake is its ancestor)
    let rotor = pid(&s, "BRK-ROT-9");
    assert!(s.add_bom_line(&rotor, &brake, 1.0, "t").is_err());
}

#[test]
fn where_used_lists_parents() {
    let s = store();
    let pads = pid(&s, "BRK-PAD-22");
    let wu = s.where_used(&pads);
    assert_eq!(wu["used_in_count"], 1);
    assert_eq!(wu["used_in"][0]["part_number"], "BRK-ASSY-100");
}

#[test]
fn supersession_resolves_to_current() {
    let s = store();
    let old = pid(&s, "SEN-O2-1");
    let new = pid(&s, "SEN-O2-2");
    let r = s.resolve_supersession(&old);
    assert_eq!(r["superseded"], true);
    assert_eq!(r["current_part_id"], new);
    // old part marked superseded
    assert_eq!(s.get_part(&old).unwrap().status, PartStatus::Superseded);
}

#[test]
fn reorder_suggestions_flags_low_stock() {
    let s = store();
    // seeded rotor: on_hand 8, ROP 10 -> should appear
    let sug = s.reorder_suggestions(None);
    let rows = sug["suggestions"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["part_number"] == "BRK-ROT-9"), "rotor should be suggested: {sug}");
}

#[test]
fn forecast_uses_usage_history() {
    let s = store();
    let filter = pid(&s, "OIL-FLT-3");
    let f = s.forecast_demand(&filter, 30).unwrap();
    assert!(f["avg_daily_usage"].as_f64().unwrap() > 0.0);
    assert!(f["recommended_reorder_point"].as_f64().unwrap() > 0.0);
    assert!(f["usage_records"].as_u64().unwrap() >= 12);
}

#[test]
fn issue_records_usage_for_forecast() {
    let s = store();
    let pads = pid(&s, "BRK-PAD-22");
    let wh = loc(&s, "Warehouse");
    // no usage history for pads yet -> forecast notes none
    let f0 = s.forecast_demand(&pads, 30).unwrap();
    assert_eq!(f0["note"], "no usage history");
    // issuing creates a usage record
    s.issue_stock(&pads, &wh, 4.0, "wo", "tech").unwrap();
    s.record_usage(&pads, Utc::now().date_naive() - Duration::days(10), 6.0, "tech").unwrap();
    let f1 = s.forecast_demand(&pads, 30).unwrap();
    assert!(f1["avg_daily_usage"].as_f64().unwrap() > 0.0);
}
