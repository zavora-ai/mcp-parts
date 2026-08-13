# Changelog

## [1.1.0] - 2026-08-13

### Changed
- Upgraded to rmcp 3.1.2 and raised the minimum supported Rust version to 1.94.1.
- Added MCP 2026-07-28 stateless request handling while retaining MCP 2025-11-25 initialization compatibility.

### Added
- Per-request identity and protocol metadata, on-demand discovery/cache hints, and the configured Tasks and sealed MRTR approval policies.

## [1.0.0] - 2026-06-10

Initial release — a broad parts & spares platform for automotive & manufacturing agents.

### Added
- **Catalog, locations & suppliers** — parts with status/lead time/cost; multi-location stocking; suppliers
  (`create_part`, `get_part`, `list_parts`, `create_location`, `list_locations`, `create_supplier`, `list_suppliers`)
- **Stock & movements** — per-location stock with reorder policy; receipt/issue/transfer/adjust with oversell + negative-stock guards
  (`set_stock_policy`, `get_stock`, `stock_for_part`, `receive_stock`, `issue_stock`, `transfer_stock`, `adjust_stock`, `movements_for_part`)
- **Reservations** — commit available stock against a reference; cancel to release
  (`reserve_stock`, `cancel_reservation`, `list_reservations`)
- **BOM & supersession** — recursive BOM explosion with cycle guard, where-used, and supersession-chain resolution
  (`add_bom_line`, `explode_bom`, `where_used`, `add_supersession`, `resolve_supersession`)
- **Reorder & forecasting** — reorder-point suggestions and usage-based demand forecasting (avg daily usage, projected demand, recommended reorder point, days of cover, stockout risk)
  (`reorder_suggestions`, `record_usage`, `forecast_demand`, `audit_log`)
- 27 tools total; `issue_stock` and `adjust_stock` (external writes consuming/overriding physical stock) are approval-gated; full stock-movement audit trail.
- 15 tests (11 integration + 4 manifest); verified end-to-end over MCP stdio.
