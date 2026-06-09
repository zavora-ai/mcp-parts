//! Parts & spares platform domain model.
//!
//! Broad parts/inventory platform: a parts catalog, multi-location stock with
//! reorder points, bill-of-materials (and where-used), supersession chains,
//! suppliers, stock movements (receipt/issue/transfer/adjust), reservations,
//! usage history, and reorder/forecast analytics. The named agents (Parts
//! Inventory Optimizer, Predictive Fleet Maintenance, Predictive Maintenance)
//! are clients of this platform.

use chrono::{DateTime, NaiveDate, Utc};
use rmcp::schemars;
use serde::{Deserialize, Serialize};

// ─── parts ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PartStatus {
    Active,
    /// Superseded by a newer part (see SupersessionLink).
    Superseded,
    /// No longer supplied, no replacement.
    Obsolete,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Part {
    pub id: String,
    /// Manufacturer/stock part number.
    pub part_number: String,
    pub name: String,
    pub category: String,
    pub unit: String,
    pub status: PartStatus,
    /// Default supplier and lead time for replenishment math.
    pub preferred_supplier_id: Option<String>,
    pub lead_time_days: u32,
    pub unit_cost: f64,
    pub created_at: DateTime<Utc>,
}

// ─── locations & stock ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Location {
    pub id: String,
    pub name: String,
    /// warehouse | depot | van | line-side | service-center
    pub kind: String,
    pub created_at: DateTime<Utc>,
}

/// Stock of a part at a location, with reorder policy.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StockItem {
    pub part_id: String,
    pub location_id: String,
    pub on_hand: f64,
    /// Quantity reserved (committed but not yet issued).
    pub reserved: f64,
    pub reorder_point: f64,
    /// Target level to replenish up to.
    pub reorder_qty: f64,
    pub updated_at: DateTime<Utc>,
}

// ─── BOM / where-used ────────────────────────────────────────────────────────

/// A bill-of-materials edge: `parent` part is built from `qty` of `child` part.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BomLine {
    pub parent_id: String,
    pub child_id: String,
    pub qty: f64,
}

// ─── supersession ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SupersessionLink {
    /// Old part.
    pub from_id: String,
    /// Replacement part.
    pub to_id: String,
    pub note: String,
}

// ─── suppliers ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Supplier {
    pub id: String,
    pub name: String,
    pub lead_time_days: u32,
    pub created_at: DateTime<Utc>,
}

// ─── stock movements ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MovementKind {
    /// Stock in (purchase/return).
    Receipt,
    /// Stock out (consumed/installed).
    Issue,
    /// Move between locations.
    Transfer,
    /// Cycle-count / correction (can be +/-).
    Adjustment,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Movement {
    pub id: String,
    pub part_id: String,
    pub kind: MovementKind,
    pub from_location_id: Option<String>,
    pub to_location_id: Option<String>,
    /// Signed at the affected location(s); always recorded as the requested qty.
    pub qty: f64,
    pub reason: String,
    pub actor: String,
    pub at: DateTime<Utc>,
}

// ─── reservations ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReservationStatus {
    Open,
    Fulfilled,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Reservation {
    pub id: String,
    pub part_id: String,
    pub location_id: String,
    pub qty: f64,
    pub status: ReservationStatus,
    /// e.g. a work order or asset id.
    pub reference: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

// ─── usage history (for forecasting) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UsageRecord {
    pub part_id: String,
    pub date: NaiveDate,
    pub qty: f64,
}

// ─── audit trail ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AuditEntry {
    pub at: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub detail: String,
}
