//! MCP tool surface for the parts platform.
//!
//! Reads (catalog, stock, BOM, where-used, supersession, reorder, forecast) are
//! `read_only`. Most writes are `internal_write`. Two consume physical stock and
//! are gated (`requires_approval`): `issue_stock` (consumes/installs parts —
//! `external_write`) and `adjust_stock` (overrides recorded inventory).

use crate::store::PartsStore;
use crate::types::*;
use adk_mcp_sdk::{HealthCheck, HealthStatus};
use chrono::NaiveDate;
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;
use std::sync::Arc;

fn dactor() -> String { "agent".into() }
fn date(s: &Option<String>) -> Option<NaiveDate> { s.as_ref().and_then(|x| NaiveDate::parse_from_str(x, "%Y-%m-%d").ok()) }
fn today() -> NaiveDate { chrono::Utc::now().date_naive() }
fn dea() -> String { "ea".into() }
fn dlt() -> u32 { 14 }
fn dlimit() -> usize { 50 }

// ─── inputs ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreatePartInput { pub part_number: String, #[serde(default)] pub name: String, #[serde(default)] pub category: String, #[serde(default = "dea")] pub unit: String, pub preferred_supplier_id: Option<String>, #[serde(default = "dlt")] pub lead_time_days: u32, #[serde(default)] pub unit_cost: f64, #[serde(default = "dactor")] pub actor: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResolvePartInput { pub part: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PartIdInput { pub part_id: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListPartsInput { pub category: Option<String>, pub status: Option<PartStatus> }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateLocationInput { pub name: String, #[serde(default = "dwh")] pub kind: String, #[serde(default = "dactor")] pub actor: String }
fn dwh() -> String { "warehouse".into() }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateSupplierInput { pub name: String, #[serde(default = "dlt")] pub lead_time_days: u32, #[serde(default = "dactor")] pub actor: String }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetPolicyInput { pub part_id: String, pub location_id: String, pub reorder_point: f64, pub reorder_qty: f64, #[serde(default = "dactor")] pub actor: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StockAtInput { pub part_id: String, pub location_id: String }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReceiveInput { pub part_id: String, pub location_id: String, pub qty: f64, #[serde(default = "drecv")] pub reason: String, #[serde(default = "dactor")] pub actor: String }
fn drecv() -> String { "receipt".into() }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IssueInput { pub part_id: String, pub location_id: String, pub qty: f64, #[serde(default = "diss")] pub reason: String, #[serde(default = "dactor")] pub actor: String }
fn diss() -> String { "issue".into() }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TransferInput { pub part_id: String, pub from_location_id: String, pub to_location_id: String, pub qty: f64, #[serde(default = "dxfer")] pub reason: String, #[serde(default = "dactor")] pub actor: String }
fn dxfer() -> String { "transfer".into() }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AdjustInput { pub part_id: String, pub location_id: String, pub delta: f64, pub reason: String, #[serde(default = "dactor")] pub actor: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MovementsInput { pub part_id: String, #[serde(default = "dlimit")] pub limit: usize }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReserveInput { pub part_id: String, pub location_id: String, pub qty: f64, #[serde(default)] pub reference: String, #[serde(default = "dactor")] pub actor: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReservationIdInput { pub reservation_id: String, #[serde(default = "dactor")] pub actor: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListReservationsInput { pub part_id: Option<String>, #[serde(default = "dtrue")] pub open_only: bool }
fn dtrue() -> bool { true }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BomLineInput { pub parent_id: String, pub child_id: String, pub qty: f64, #[serde(default = "dactor")] pub actor: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExplodeInput { pub parent_id: String, #[serde(default = "done")] pub multiplier: f64 }
fn done() -> f64 { 1.0 }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersessionInput { pub from_id: String, pub to_id: String, #[serde(default)] pub note: String, #[serde(default = "dactor")] pub actor: String }

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReorderInput { pub location_id: Option<String> }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordUsageInput { pub part_id: String, pub date: Option<String>, pub qty: f64, #[serde(default = "dactor")] pub actor: String }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ForecastInput { pub part_id: String, #[serde(default = "dhoriz")] pub horizon_days: u32 }
fn dhoriz() -> u32 { 30 }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AuditLogInput { #[serde(default = "dlimit")] pub limit: usize }
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EmptyInput {}

// ─── server ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PartsServer { pub store: Arc<PartsStore> }

#[tool_router(server_handler)]
impl PartsServer {
    // catalog
    #[tool(description = "Create a part in the catalog.")]
    fn create_part(&self, Parameters(i): Parameters<CreatePartInput>) -> String {
        let p = self.store.create_part(&i.part_number, &i.name, &i.category, &i.unit, i.preferred_supplier_id, i.lead_time_days, i.unit_cost, &i.actor);
        serde_json::to_string_pretty(&p).unwrap()
    }

    #[tool(description = "Get a part by id or part number.")]
    fn get_part(&self, Parameters(i): Parameters<ResolvePartInput>) -> String {
        match self.store.resolve_part(&i.part) {
            Some(p) => serde_json::to_string_pretty(&p).unwrap(), None => format!("Part not found: {}", i.part) }
    }

    #[tool(description = "List parts, optionally by category and/or status.")]
    fn list_parts(&self, Parameters(i): Parameters<ListPartsInput>) -> String {
        let v = self.store.list_parts(i.category.as_deref(), i.status);
        serde_json::to_string_pretty(&serde_json::json!({"count": v.len(), "parts": v})).unwrap()
    }

    // locations & suppliers
    #[tool(description = "Create a stocking location (warehouse/depot/van/line-side/service-center).")]
    fn create_location(&self, Parameters(i): Parameters<CreateLocationInput>) -> String {
        let l = self.store.create_location(&i.name, &i.kind, &i.actor);
        serde_json::to_string_pretty(&l).unwrap()
    }

    #[tool(description = "List stocking locations.")]
    fn list_locations(&self, Parameters(_): Parameters<EmptyInput>) -> String {
        let v = self.store.list_locations();
        serde_json::to_string_pretty(&serde_json::json!({"count": v.len(), "locations": v})).unwrap()
    }

    #[tool(description = "Create a supplier.")]
    fn create_supplier(&self, Parameters(i): Parameters<CreateSupplierInput>) -> String {
        let s = self.store.create_supplier(&i.name, i.lead_time_days, &i.actor);
        serde_json::to_string_pretty(&s).unwrap()
    }

    #[tool(description = "List suppliers.")]
    fn list_suppliers(&self, Parameters(_): Parameters<EmptyInput>) -> String {
        let v = self.store.list_suppliers();
        serde_json::to_string_pretty(&serde_json::json!({"count": v.len(), "suppliers": v})).unwrap()
    }

    // stock & policy
    #[tool(description = "Set the reorder policy (reorder point + reorder qty) for a part at a location.")]
    fn set_stock_policy(&self, Parameters(i): Parameters<SetPolicyInput>) -> String {
        match self.store.set_stock_policy(&i.part_id, &i.location_id, i.reorder_point, i.reorder_qty, &i.actor) {
            Ok(s) => serde_json::to_string_pretty(&s).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Get stock of a part at a location (on hand, reserved, policy).")]
    fn get_stock(&self, Parameters(i): Parameters<StockAtInput>) -> String {
        match self.store.get_stock(&i.part_id, &i.location_id) {
            Some(s) => serde_json::to_string_pretty(&s).unwrap(), None => format!("No stock of {} at {}", i.part_id, i.location_id) }
    }

    #[tool(description = "Stock of a part across all locations, plus total available (on hand minus reserved).")]
    fn stock_for_part(&self, Parameters(i): Parameters<PartIdInput>) -> String {
        let v = self.store.stock_for_part(&i.part_id);
        serde_json::to_string_pretty(&serde_json::json!({"part_id": i.part_id, "available_total": self.store.available_total(&i.part_id), "rows": v})).unwrap()
    }

    // movements
    #[tool(description = "Receive stock into a location (purchase/return).")]
    fn receive_stock(&self, Parameters(i): Parameters<ReceiveInput>) -> String {
        match self.store.receive_stock(&i.part_id, &i.location_id, i.qty, &i.reason, &i.actor) {
            Ok(m) => serde_json::to_string_pretty(&m).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Issue (consume/install) stock from a location. Refuses to oversell available stock. Records usage for forecasting. External write — gated.")]
    fn issue_stock(&self, Parameters(i): Parameters<IssueInput>) -> String {
        match self.store.issue_stock(&i.part_id, &i.location_id, i.qty, &i.reason, &i.actor) {
            Ok(m) => serde_json::to_string_pretty(&m).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Transfer stock between locations.")]
    fn transfer_stock(&self, Parameters(i): Parameters<TransferInput>) -> String {
        match self.store.transfer_stock(&i.part_id, &i.from_location_id, &i.to_location_id, i.qty, &i.reason, &i.actor) {
            Ok(m) => serde_json::to_string_pretty(&m).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Adjust stock by a signed delta (cycle count / correction). Overrides recorded inventory — gated.")]
    fn adjust_stock(&self, Parameters(i): Parameters<AdjustInput>) -> String {
        match self.store.adjust_stock(&i.part_id, &i.location_id, i.delta, &i.reason, &i.actor) {
            Ok(m) => serde_json::to_string_pretty(&m).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Recent stock movements for a part (newest first).")]
    fn movements_for_part(&self, Parameters(i): Parameters<MovementsInput>) -> String {
        let v = self.store.movements_for_part(&i.part_id, i.limit);
        serde_json::to_string_pretty(&serde_json::json!({"count": v.len(), "movements": v})).unwrap()
    }

    // reservations
    #[tool(description = "Reserve available stock against a reference (work order/asset). Increases reserved without removing on-hand.")]
    fn reserve_stock(&self, Parameters(i): Parameters<ReserveInput>) -> String {
        match self.store.reserve_stock(&i.part_id, &i.location_id, i.qty, &i.reference, &i.actor) {
            Ok(r) => serde_json::to_string_pretty(&r).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Cancel a reservation, releasing the reserved quantity.")]
    fn cancel_reservation(&self, Parameters(i): Parameters<ReservationIdInput>) -> String {
        match self.store.cancel_reservation(&i.reservation_id, &i.actor) {
            Ok(r) => serde_json::to_string_pretty(&r).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "List reservations, optionally by part (open_only defaults true).")]
    fn list_reservations(&self, Parameters(i): Parameters<ListReservationsInput>) -> String {
        let v = self.store.list_reservations(i.part_id.as_deref(), i.open_only);
        serde_json::to_string_pretty(&serde_json::json!({"count": v.len(), "reservations": v})).unwrap()
    }

    // BOM / where-used
    #[tool(description = "Add a bill-of-materials line: parent is built from qty of child. Rejects self-reference and cycles.")]
    fn add_bom_line(&self, Parameters(i): Parameters<BomLineInput>) -> String {
        match self.store.add_bom_line(&i.parent_id, &i.child_id, i.qty, &i.actor) {
            Ok(l) => serde_json::to_string_pretty(&l).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Explode a BOM into total component quantities (recursive), scaled by a multiplier.")]
    fn explode_bom(&self, Parameters(i): Parameters<ExplodeInput>) -> String {
        serde_json::to_string_pretty(&self.store.explode_bom(&i.parent_id, i.multiplier)).unwrap()
    }

    #[tool(description = "Where-used: parent assemblies that consume a given part.")]
    fn where_used(&self, Parameters(i): Parameters<PartIdInput>) -> String {
        serde_json::to_string_pretty(&self.store.where_used(&i.part_id)).unwrap()
    }

    // supersession
    #[tool(description = "Record that one part supersedes another (marks the old part superseded).")]
    fn add_supersession(&self, Parameters(i): Parameters<SupersessionInput>) -> String {
        match self.store.add_supersession(&i.from_id, &i.to_id, &i.note, &i.actor) {
            Ok(l) => serde_json::to_string_pretty(&l).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Resolve a part's supersession chain to its current active replacement.")]
    fn resolve_supersession(&self, Parameters(i): Parameters<PartIdInput>) -> String {
        serde_json::to_string_pretty(&self.store.resolve_supersession(&i.part_id)).unwrap()
    }

    // reorder & forecast
    #[tool(description = "Reorder suggestions: stock at/below reorder point with suggested order qty. Powers the Parts Inventory Optimizer.")]
    fn reorder_suggestions(&self, Parameters(i): Parameters<ReorderInput>) -> String {
        serde_json::to_string_pretty(&self.store.reorder_suggestions(i.location_id.as_deref())).unwrap()
    }

    #[tool(description = "Record a usage/consumption event for a part (feeds demand forecasting).")]
    fn record_usage(&self, Parameters(i): Parameters<RecordUsageInput>) -> String {
        let d = date(&i.date).unwrap_or_else(today);
        match self.store.record_usage(&i.part_id, d, i.qty, &i.actor) {
            Ok(u) => serde_json::to_string_pretty(&u).unwrap(), Err(e) => format!("Error: {e}") }
    }

    #[tool(description = "Demand forecast from usage history: avg daily usage, projected demand, recommended reorder point, days of cover, and stockout risk vs lead time. Powers the Predictive (Fleet) Maintenance agents.")]
    fn forecast_demand(&self, Parameters(i): Parameters<ForecastInput>) -> String {
        match self.store.forecast_demand(&i.part_id, i.horizon_days) {
            Some(v) => serde_json::to_string_pretty(&v).unwrap(), None => format!("Part not found: {}", i.part_id) }
    }

    #[tool(description = "Recent stock/operations audit-trail entries (most recent first).")]
    fn audit_log(&self, Parameters(i): Parameters<AuditLogInput>) -> String {
        let v = self.store.audit_log(i.limit);
        serde_json::to_string_pretty(&serde_json::json!({"count": v.len(), "entries": v})).unwrap()
    }
}

#[async_trait::async_trait]
impl HealthCheck for PartsServer {
    async fn check_health(&self) -> HealthStatus {
        HealthStatus { healthy: true, message: Some("operational".into()), latency_ms: Some(1) }
    }
}
