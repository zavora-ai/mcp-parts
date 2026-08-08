# Parts MCP Server

[![Crates.io](https://img.shields.io/crates/v/mcp-parts.svg)](https://crates.io/crates/mcp-parts)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![ADK-Rust Enterprise](https://img.shields.io/badge/ADK--Rust-Enterprise-purple.svg)](https://enterprise.adk-rust.com)
[![Registry Ready](https://img.shields.io/badge/ADK_Registry-Ready-green.svg)](https://www.zavora.ai)

A parts & spares platform for [ADK-Rust Enterprise](https://enterprise.adk-rust.com) automotive and manufacturing agents. 27 MCP tools covering a parts catalog, **multi-location inventory with reorder points**, **bill-of-materials & where-used**, **supersession chains**, suppliers, stock movements, reservations, reorder suggestions, and **usage-based demand forecasting** — over a full stock-movement audit trail.

## A platform, not a point solution

This is modeled as a general parts/spares inventory backbone (the layer behind an EAM/CMMS or DMS parts module), so a range of inventory and maintenance agents are clients of one shared system:

| Agent | Domain | Uses |
|-------|--------|------|
| **Parts Inventory Optimizer** | automotive | `reorder_suggestions`, `stock_for_part`, `explode_bom`, `transfer_stock` |
| **Predictive Fleet Maintenance** | automotive | `forecast_demand`, `resolve_supersession`, `reserve_stock`, `issue_stock` |
| **Predictive Maintenance Agent** | manufacturing | `forecast_demand`, `where_used`, `record_usage`, `reorder_suggestions` |

## Architecture

<p align="center">
  <img src="https://raw.githubusercontent.com/zavora-ai/mcp-parts/main/docs/architecture.svg" alt="Parts MCP Architecture" width="780"/>
</p>

## Capabilities

- **Catalog** — parts with part number, category, unit, cost, preferred supplier, and lead time; lifecycle status (active/superseded/obsolete).
- **Multi-location inventory** — stock per part per location with on-hand, reserved, and a reorder policy (point + qty). `stock_for_part` totals availability across locations.
- **Stock movements** — `receive_stock`, `issue_stock`, `transfer_stock`, `adjust_stock`, each recorded immutably. Issue/transfer/reserve refuse to oversell available stock; adjustments can't drive on-hand negative.
- **Reservations** — commit available stock against a work order/asset without removing on-hand; cancel to release.
- **BOM & where-used** — bill-of-materials with **recursive explosion** (qty-multiplied) and cycle prevention; reverse `where_used` lookup.
- **Supersession** — old→new replacement chains; `resolve_supersession` follows the chain to the current active part.
- **Reorder & forecasting** — `reorder_suggestions` flags stock at/below its reorder point; `forecast_demand` derives average daily usage from history, projects demand, recommends a reorder point (lead-time demand + safety stock), and reports days-of-cover and **stockout risk** vs. lead time.

## Governance posture

- **Two writes consume physical stock and are gated** (`requires_approval`, `external_write`): `issue_stock` (parts consumed/installed — also records usage) and `adjust_stock` (overrides recorded inventory). Receiving and transferring are normal internal writes.
- **Movement integrity** — oversell and negative-stock guards on every movement; every movement and adjustment is on the audit trail (`audit_log`).
- **Reads are `read_only`**. Sample data is fictitious.

## Tools (27)

### Catalog, Locations & Suppliers (7)
`create_part` · `get_part` · `list_parts` · `create_location` · `list_locations` · `create_supplier` · `list_suppliers`

### Stock & Movements (8)
`set_stock_policy` · `get_stock` · `stock_for_part` · `receive_stock` · `issue_stock` (gated, external) · `transfer_stock` · `adjust_stock` (gated, external) · `movements_for_part`

### Reservations (3)
`reserve_stock` · `cancel_reservation` · `list_reservations`

### BOM & Supersession (5)
`add_bom_line` · `explode_bom` · `where_used` · `add_supersession` · `resolve_supersession`

### Reorder, Forecast & Audit (4)
`reorder_suggestions` · `record_usage` · `forecast_demand` · `audit_log`

## Example

```jsonc
// Inventory Optimizer: what needs ordering, and what a build consumes
{"name": "reorder_suggestions", "arguments": {}}
{"name": "explode_bom", "arguments": {"parent_id": "PRT-1004", "multiplier": 5}}

// Predictive Maintenance: forecast a consumable
{"name": "forecast_demand", "arguments": {"part_id": "PRT-1007", "horizon_days": 30}}

// Fleet Maintenance: resolve a superseded part, then issue (gated)
{"name": "resolve_supersession", "arguments": {"part_id": "PRT-1009"}}
{"name": "issue_stock", "arguments": {"part_id": "PRT-1002", "location_id": "LOC-1003", "qty": 4, "reason": "wo-1"}}
```

## Install & run

```bash
cargo install mcp-parts
mcp-parts            # serves MCP over stdio
```

Or build from source:

```bash
git clone https://github.com/zavora-ai/mcp-parts
cd mcp-parts && cargo build --release
./target/release/mcp-parts
```

## Registry manifest

```toml
server_id = "mcp_parts"
display_name = "Parts & Spares"
version = "1.0.0"
domain = "automotive"
risk_level = "high"
writes_allowed = "gated"
```

The full [`mcp-server.toml`](mcp-server.toml) declares all 27 tools with risk classes and approval gates for registry onboarding.

## License

Apache-2.0

## rmcp and MCP compatibility

This server is built with [`rmcp` 3.1.2](https://github.com/modelcontextprotocol/rust-sdk/releases/tag/rmcp-v3.1.2) and requires Rust 1.88 or newer. The rmcp 3 rollout retains legacy MCP initialization compatibility and targets MCP protocol revisions `2025-11-25` and `2026-07-28`.
