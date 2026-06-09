//! In-memory parts store with seeded data and engines.
//!
//! Thread-safe via per-collection `Mutex`. IDs come from a monotonic sequence
//! (`PREFIX-{n}` from 1000). Every stock-affecting action appends to an audit
//! trail. Engines: stock movements (receipt/issue/transfer/adjust), reorder
//! suggestions, BOM explosion + where-used, supersession resolution, and
//! usage-based demand forecasting.

use crate::types::*;
use chrono::{Duration, NaiveDate, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

type StockKey = (String, String); // (part_id, location_id)

pub struct PartsStore {
    parts: Mutex<HashMap<String, Part>>,
    locations: Mutex<HashMap<String, Location>>,
    stock: Mutex<HashMap<StockKey, StockItem>>,
    bom: Mutex<Vec<BomLine>>,
    supersessions: Mutex<Vec<SupersessionLink>>,
    suppliers: Mutex<HashMap<String, Supplier>>,
    movements: Mutex<Vec<Movement>>,
    reservations: Mutex<HashMap<String, Reservation>>,
    usage: Mutex<Vec<UsageRecord>>,
    audit_log: Mutex<Vec<AuditEntry>>,
    seq: Mutex<u64>,
}

impl Default for PartsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PartsStore {
    pub fn new() -> Self {
        let s = PartsStore {
            parts: Mutex::new(HashMap::new()),
            locations: Mutex::new(HashMap::new()),
            stock: Mutex::new(HashMap::new()),
            bom: Mutex::new(Vec::new()),
            supersessions: Mutex::new(Vec::new()),
            suppliers: Mutex::new(HashMap::new()),
            movements: Mutex::new(Vec::new()),
            reservations: Mutex::new(HashMap::new()),
            usage: Mutex::new(Vec::new()),
            audit_log: Mutex::new(Vec::new()),
            seq: Mutex::new(1000),
        };
        s.seed();
        s
    }

    fn next(&self, prefix: &str) -> String {
        let mut n = self.seq.lock().unwrap();
        *n += 1;
        format!("{prefix}-{n}")
    }

    fn audit(&self, actor: &str, action: &str, detail: impl Into<String>) {
        self.audit_log.lock().unwrap().push(AuditEntry { at: Utc::now(), actor: actor.to_string(), action: action.to_string(), detail: detail.into() });
    }

    pub fn part_exists(&self, id: &str) -> bool { self.parts.lock().unwrap().contains_key(id) }
    pub fn location_exists(&self, id: &str) -> bool { self.locations.lock().unwrap().contains_key(id) }

    // ─── catalog ─────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn create_part(&self, part_number: &str, name: &str, category: &str, unit: &str, supplier_id: Option<String>, lead_time_days: u32, unit_cost: f64, actor: &str) -> Part {
        let p = Part {
            id: self.next("PRT"),
            part_number: part_number.to_string(),
            name: name.to_string(),
            category: category.to_string(),
            unit: unit.to_string(),
            status: PartStatus::Active,
            preferred_supplier_id: supplier_id,
            lead_time_days,
            unit_cost,
            created_at: Utc::now(),
        };
        self.parts.lock().unwrap().insert(p.id.clone(), p.clone());
        self.audit(actor, "create_part", p.part_number.clone());
        p
    }

    pub fn get_part(&self, id: &str) -> Option<Part> {
        self.parts.lock().unwrap().get(id).cloned()
    }

    /// Resolve by id or part number.
    pub fn resolve_part(&self, id_or_pn: &str) -> Option<Part> {
        let parts = self.parts.lock().unwrap();
        parts.get(id_or_pn).cloned().or_else(|| parts.values().find(|p| p.part_number.eq_ignore_ascii_case(id_or_pn)).cloned())
    }

    pub fn list_parts(&self, category: Option<&str>, status: Option<PartStatus>) -> Vec<Part> {
        let mut v: Vec<Part> = self.parts.lock().unwrap().values()
            .filter(|p| category.is_none_or(|c| p.category.eq_ignore_ascii_case(c)))
            .filter(|p| status.is_none_or(|s| p.status == s))
            .cloned().collect();
        v.sort_by(|a, b| a.part_number.cmp(&b.part_number));
        v
    }

    // ─── locations ───────────────────────────────────────────────────────

    pub fn create_location(&self, name: &str, kind: &str, actor: &str) -> Location {
        let l = Location { id: self.next("LOC"), name: name.to_string(), kind: kind.to_string(), created_at: Utc::now() };
        self.locations.lock().unwrap().insert(l.id.clone(), l.clone());
        self.audit(actor, "create_location", l.id.clone());
        l
    }

    pub fn list_locations(&self) -> Vec<Location> {
        let mut v: Vec<Location> = self.locations.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    // ─── suppliers ─────────────────────────────────────────────────────────

    pub fn create_supplier(&self, name: &str, lead_time_days: u32, actor: &str) -> Supplier {
        let s = Supplier { id: self.next("SUP"), name: name.to_string(), lead_time_days, created_at: Utc::now() };
        self.suppliers.lock().unwrap().insert(s.id.clone(), s.clone());
        self.audit(actor, "create_supplier", s.id.clone());
        s
    }

    pub fn list_suppliers(&self) -> Vec<Supplier> {
        let mut v: Vec<Supplier> = self.suppliers.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    // ─── stock & policy ──────────────────────────────────────────────────

    pub fn set_stock_policy(&self, part_id: &str, location_id: &str, reorder_point: f64, reorder_qty: f64, actor: &str) -> Result<StockItem, String> {
        if !self.part_exists(part_id) { return Err(format!("Part not found: {part_id}")); }
        if !self.location_exists(location_id) { return Err(format!("Location not found: {location_id}")); }
        let mut stock = self.stock.lock().unwrap();
        let item = stock.entry((part_id.to_string(), location_id.to_string())).or_insert_with(|| StockItem {
            part_id: part_id.to_string(), location_id: location_id.to_string(), on_hand: 0.0, reserved: 0.0, reorder_point: 0.0, reorder_qty: 0.0, updated_at: Utc::now(),
        });
        item.reorder_point = reorder_point;
        item.reorder_qty = reorder_qty;
        item.updated_at = Utc::now();
        let out = item.clone();
        drop(stock);
        self.audit(actor, "set_stock_policy", format!("{part_id}@{location_id} rop={reorder_point}"));
        Ok(out)
    }

    pub fn get_stock(&self, part_id: &str, location_id: &str) -> Option<StockItem> {
        self.stock.lock().unwrap().get(&(part_id.to_string(), location_id.to_string())).cloned()
    }

    /// All stock rows for a part across locations.
    pub fn stock_for_part(&self, part_id: &str) -> Vec<StockItem> {
        let mut v: Vec<StockItem> = self.stock.lock().unwrap().values().filter(|s| s.part_id == part_id).cloned().collect();
        v.sort_by(|a, b| a.location_id.cmp(&b.location_id));
        v
    }

    /// Total available (on_hand - reserved) for a part across all locations.
    pub fn available_total(&self, part_id: &str) -> f64 {
        self.stock.lock().unwrap().values().filter(|s| s.part_id == part_id).map(|s| (s.on_hand - s.reserved).max(0.0)).sum()
    }

    fn ensure_stock_row<'a>(stock: &'a mut HashMap<StockKey, StockItem>, part_id: &str, location_id: &str) -> &'a mut StockItem {
        stock.entry((part_id.to_string(), location_id.to_string())).or_insert_with(|| StockItem {
            part_id: part_id.to_string(), location_id: location_id.to_string(), on_hand: 0.0, reserved: 0.0, reorder_point: 0.0, reorder_qty: 0.0, updated_at: Utc::now(),
        })
    }

    // ─── movements ───────────────────────────────────────────────────────

    /// Receive stock into a location (purchase/return).
    pub fn receive_stock(&self, part_id: &str, location_id: &str, qty: f64, reason: &str, actor: &str) -> Result<Movement, String> {
        if !self.part_exists(part_id) { return Err(format!("Part not found: {part_id}")); }
        if !self.location_exists(location_id) { return Err(format!("Location not found: {location_id}")); }
        if qty <= 0.0 { return Err("receipt qty must be positive".into()); }
        {
            let mut stock = self.stock.lock().unwrap();
            let item = Self::ensure_stock_row(&mut stock, part_id, location_id);
            item.on_hand += qty;
            item.updated_at = Utc::now();
        }
        Ok(self.record_movement(part_id, MovementKind::Receipt, None, Some(location_id.to_string()), qty, reason, actor))
    }

    /// Issue (consume) stock from a location. Refuses to oversell available stock.
    pub fn issue_stock(&self, part_id: &str, location_id: &str, qty: f64, reason: &str, actor: &str) -> Result<Movement, String> {
        if qty <= 0.0 { return Err("issue qty must be positive".into()); }
        {
            let mut stock = self.stock.lock().unwrap();
            let item = stock.get_mut(&(part_id.to_string(), location_id.to_string())).ok_or_else(|| format!("No stock of {part_id} at {location_id}"))?;
            let available = item.on_hand - item.reserved;
            if qty > available {
                return Err(format!("insufficient available stock: {available} < {qty} (on_hand {}, reserved {})", item.on_hand, item.reserved));
            }
            item.on_hand -= qty;
            item.updated_at = Utc::now();
        }
        // Record usage for forecasting.
        self.usage.lock().unwrap().push(UsageRecord { part_id: part_id.to_string(), date: Utc::now().date_naive(), qty });
        Ok(self.record_movement(part_id, MovementKind::Issue, Some(location_id.to_string()), None, qty, reason, actor))
    }

    /// Transfer stock between locations.
    pub fn transfer_stock(&self, part_id: &str, from_id: &str, to_id: &str, qty: f64, reason: &str, actor: &str) -> Result<Movement, String> {
        if qty <= 0.0 { return Err("transfer qty must be positive".into()); }
        if !self.location_exists(to_id) { return Err(format!("Location not found: {to_id}")); }
        {
            let mut stock = self.stock.lock().unwrap();
            let from = stock.get_mut(&(part_id.to_string(), from_id.to_string())).ok_or_else(|| format!("No stock of {part_id} at {from_id}"))?;
            let available = from.on_hand - from.reserved;
            if qty > available { return Err(format!("insufficient available stock to transfer: {available} < {qty}")); }
            from.on_hand -= qty;
            from.updated_at = Utc::now();
            let to = Self::ensure_stock_row(&mut stock, part_id, to_id);
            to.on_hand += qty;
            to.updated_at = Utc::now();
        }
        Ok(self.record_movement(part_id, MovementKind::Transfer, Some(from_id.to_string()), Some(to_id.to_string()), qty, reason, actor))
    }

    /// Adjust stock by a signed delta (cycle count / correction).
    pub fn adjust_stock(&self, part_id: &str, location_id: &str, delta: f64, reason: &str, actor: &str) -> Result<Movement, String> {
        if !self.part_exists(part_id) { return Err(format!("Part not found: {part_id}")); }
        if !self.location_exists(location_id) { return Err(format!("Location not found: {location_id}")); }
        {
            let mut stock = self.stock.lock().unwrap();
            let item = Self::ensure_stock_row(&mut stock, part_id, location_id);
            if item.on_hand + delta < 0.0 { return Err(format!("adjustment would drive on_hand negative ({} + {delta})", item.on_hand)); }
            item.on_hand += delta;
            item.updated_at = Utc::now();
        }
        Ok(self.record_movement(part_id, MovementKind::Adjustment, None, Some(location_id.to_string()), delta, reason, actor))
    }

    fn record_movement(&self, part_id: &str, kind: MovementKind, from: Option<String>, to: Option<String>, qty: f64, reason: &str, actor: &str) -> Movement {
        let m = Movement { id: self.next("MOV"), part_id: part_id.to_string(), kind, from_location_id: from, to_location_id: to, qty, reason: reason.to_string(), actor: actor.to_string(), at: Utc::now() };
        self.movements.lock().unwrap().push(m.clone());
        self.audit(actor, "stock_movement", format!("{:?} {part_id} {qty}", kind));
        m
    }

    pub fn movements_for_part(&self, part_id: &str, limit: usize) -> Vec<Movement> {
        let mv = self.movements.lock().unwrap();
        let mut v: Vec<Movement> = mv.iter().filter(|m| m.part_id == part_id).cloned().collect();
        v.sort_by(|a, b| b.at.cmp(&a.at));
        v.truncate(limit);
        v
    }

    // ─── reservations ────────────────────────────────────────────────────

    pub fn reserve_stock(&self, part_id: &str, location_id: &str, qty: f64, reference: &str, actor: &str) -> Result<Reservation, String> {
        if qty <= 0.0 { return Err("reservation qty must be positive".into()); }
        {
            let mut stock = self.stock.lock().unwrap();
            let item = stock.get_mut(&(part_id.to_string(), location_id.to_string())).ok_or_else(|| format!("No stock of {part_id} at {location_id}"))?;
            let available = item.on_hand - item.reserved;
            if qty > available { return Err(format!("insufficient available stock to reserve: {available} < {qty}")); }
            item.reserved += qty;
            item.updated_at = Utc::now();
        }
        let r = Reservation { id: self.next("RSV"), part_id: part_id.to_string(), location_id: location_id.to_string(), qty, status: ReservationStatus::Open, reference: reference.to_string(), created_by: actor.to_string(), created_at: Utc::now() };
        self.reservations.lock().unwrap().insert(r.id.clone(), r.clone());
        self.audit(actor, "reserve_stock", format!("{} {part_id} {qty}", r.id));
        Ok(r)
    }

    /// Cancel a reservation, releasing the reserved quantity.
    pub fn cancel_reservation(&self, reservation_id: &str, actor: &str) -> Result<Reservation, String> {
        let mut reservations = self.reservations.lock().unwrap();
        let r = reservations.get_mut(reservation_id).ok_or_else(|| format!("Reservation not found: {reservation_id}"))?;
        if r.status != ReservationStatus::Open { return Err(format!("reservation {reservation_id} is {:?}", r.status)); }
        r.status = ReservationStatus::Cancelled;
        let (part_id, location_id, qty) = (r.part_id.clone(), r.location_id.clone(), r.qty);
        let out = r.clone();
        drop(reservations);
        if let Some(item) = self.stock.lock().unwrap().get_mut(&(part_id, location_id)) {
            item.reserved = (item.reserved - qty).max(0.0);
            item.updated_at = Utc::now();
        }
        self.audit(actor, "cancel_reservation", reservation_id.to_string());
        Ok(out)
    }

    pub fn list_reservations(&self, part_id: Option<&str>, open_only: bool) -> Vec<Reservation> {
        let mut v: Vec<Reservation> = self.reservations.lock().unwrap().values()
            .filter(|r| part_id.is_none_or(|p| r.part_id == p))
            .filter(|r| !open_only || r.status == ReservationStatus::Open)
            .cloned().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    // ─── BOM / where-used ────────────────────────────────────────────────

    pub fn add_bom_line(&self, parent_id: &str, child_id: &str, qty: f64, actor: &str) -> Result<BomLine, String> {
        if !self.part_exists(parent_id) { return Err(format!("Parent part not found: {parent_id}")); }
        if !self.part_exists(child_id) { return Err(format!("Child part not found: {child_id}")); }
        if parent_id == child_id { return Err("a part cannot be its own component".into()); }
        // Prevent simple cycles (child already an ancestor of parent).
        if self.is_descendant(child_id, parent_id) {
            return Err(format!("adding {child_id} under {parent_id} would create a BOM cycle"));
        }
        let line = BomLine { parent_id: parent_id.to_string(), child_id: child_id.to_string(), qty };
        self.bom.lock().unwrap().push(line.clone());
        self.audit(actor, "add_bom_line", format!("{parent_id} -> {child_id} x{qty}"));
        Ok(line)
    }

    /// True if `target` appears anywhere beneath `root` in the BOM.
    fn is_descendant(&self, root: &str, target: &str) -> bool {
        let bom = self.bom.lock().unwrap();
        let mut stack = vec![root.to_string()];
        let mut seen = HashSet::new();
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur.clone()) { continue; }
            for l in bom.iter().filter(|l| l.parent_id == cur) {
                if l.child_id == target { return true; }
                stack.push(l.child_id.clone());
            }
        }
        false
    }

    /// Explode a BOM into total component quantities (recursive, qty-multiplied).
    pub fn explode_bom(&self, parent_id: &str, multiplier: f64) -> serde_json::Value {
        let bom = self.bom.lock().unwrap();
        let mut totals: HashMap<String, f64> = HashMap::new();
        let mut stack = vec![(parent_id.to_string(), multiplier)];
        let mut seen = HashSet::new();
        while let Some((cur, mult)) = stack.pop() {
            // Guard against cycles defensively.
            if !seen.insert(cur.clone()) && cur == parent_id { continue; }
            for l in bom.iter().filter(|l| l.parent_id == cur) {
                *totals.entry(l.child_id.clone()).or_insert(0.0) += l.qty * mult;
                stack.push((l.child_id.clone(), l.qty * mult));
            }
        }
        drop(bom);
        let parts = self.parts.lock().unwrap();
        let mut components: Vec<serde_json::Value> = totals.iter().map(|(id, qty)| {
            let pn = parts.get(id).map(|p| p.part_number.clone()).unwrap_or_default();
            serde_json::json!({"part_id": id, "part_number": pn, "total_qty": (qty*1000.0).round()/1000.0})
        }).collect();
        components.sort_by(|a, b| a["part_number"].as_str().cmp(&b["part_number"].as_str()));
        serde_json::json!({"parent_id": parent_id, "multiplier": multiplier, "component_count": components.len(), "components": components})
    }

    /// Where-used: direct parents that consume this part.
    pub fn where_used(&self, child_id: &str) -> serde_json::Value {
        let bom = self.bom.lock().unwrap();
        let parts = self.parts.lock().unwrap();
        let mut rows: Vec<serde_json::Value> = bom.iter().filter(|l| l.child_id == child_id).map(|l| {
            let pn = parts.get(&l.parent_id).map(|p| p.part_number.clone()).unwrap_or_default();
            serde_json::json!({"parent_id": l.parent_id, "part_number": pn, "qty_per": l.qty})
        }).collect();
        rows.sort_by(|a, b| a["part_number"].as_str().cmp(&b["part_number"].as_str()));
        serde_json::json!({"child_id": child_id, "used_in_count": rows.len(), "used_in": rows})
    }

    // ─── supersession ────────────────────────────────────────────────────

    pub fn add_supersession(&self, from_id: &str, to_id: &str, note: &str, actor: &str) -> Result<SupersessionLink, String> {
        if !self.part_exists(from_id) { return Err(format!("Part not found: {from_id}")); }
        if !self.part_exists(to_id) { return Err(format!("Part not found: {to_id}")); }
        if from_id == to_id { return Err("a part cannot supersede itself".into()); }
        let link = SupersessionLink { from_id: from_id.to_string(), to_id: to_id.to_string(), note: note.to_string() };
        self.supersessions.lock().unwrap().push(link.clone());
        // Mark the old part superseded.
        if let Some(p) = self.parts.lock().unwrap().get_mut(from_id) { p.status = PartStatus::Superseded; }
        self.audit(actor, "add_supersession", format!("{from_id} -> {to_id}"));
        Ok(link)
    }

    /// Follow the supersession chain to the current active replacement.
    pub fn resolve_supersession(&self, part_id: &str) -> serde_json::Value {
        let links = self.supersessions.lock().unwrap();
        let mut chain = vec![part_id.to_string()];
        let mut cur = part_id.to_string();
        let mut seen = HashSet::new();
        while seen.insert(cur.clone()) {
            match links.iter().find(|l| l.from_id == cur) {
                Some(l) => { chain.push(l.to_id.clone()); cur = l.to_id.clone(); }
                None => break,
            }
        }
        let current = chain.last().cloned().unwrap_or_else(|| part_id.to_string());
        let superseded = current != part_id;
        serde_json::json!({"part_id": part_id, "current_part_id": current, "superseded": superseded, "chain": chain})
    }

    // ─── reorder & forecast ──────────────────────────────────────────────

    /// Reorder suggestions: every stock row at/below its reorder point. Powers
    /// the Parts Inventory Optimizer.
    pub fn reorder_suggestions(&self, location_id: Option<&str>) -> serde_json::Value {
        let stock = self.stock.lock().unwrap();
        let parts = self.parts.lock().unwrap();
        let mut rows: Vec<serde_json::Value> = stock.values()
            .filter(|s| location_id.is_none_or(|l| s.location_id == l))
            .filter(|s| s.reorder_point > 0.0 && (s.on_hand - s.reserved) <= s.reorder_point)
            .map(|s| {
                let p = parts.get(&s.part_id);
                let suggest = s.reorder_qty.max(s.reorder_point - (s.on_hand - s.reserved));
                serde_json::json!({
                    "part_id": s.part_id,
                    "part_number": p.map(|x| x.part_number.clone()),
                    "location_id": s.location_id,
                    "available": (s.on_hand - s.reserved),
                    "reorder_point": s.reorder_point,
                    "suggested_order_qty": (suggest*1000.0).round()/1000.0,
                    "lead_time_days": p.map(|x| x.lead_time_days),
                })
            }).collect();
        rows.sort_by(|a, b| a["part_number"].as_str().cmp(&b["part_number"].as_str()));
        serde_json::json!({"count": rows.len(), "suggestions": rows})
    }

    pub fn record_usage(&self, part_id: &str, date: NaiveDate, qty: f64, actor: &str) -> Result<UsageRecord, String> {
        if !self.part_exists(part_id) { return Err(format!("Part not found: {part_id}")); }
        let u = UsageRecord { part_id: part_id.to_string(), date, qty };
        self.usage.lock().unwrap().push(u.clone());
        self.audit(actor, "record_usage", format!("{part_id} {qty}@{date}"));
        Ok(u)
    }

    /// Demand forecast from usage history: average daily usage, projected demand
    /// over the part's lead time, a reorder-point recommendation (lead-time demand
    /// + safety stock), and days-of-cover at current availability. Powers the
    /// predictive-maintenance agents.
    pub fn forecast_demand(&self, part_id: &str, horizon_days: u32) -> Option<serde_json::Value> {
        let part = self.get_part(part_id)?;
        let usage = self.usage.lock().unwrap();
        let records: Vec<&UsageRecord> = usage.iter().filter(|u| u.part_id == part_id).collect();
        if records.is_empty() {
            return Some(serde_json::json!({"part_id": part_id, "note": "no usage history"}));
        }
        let total: f64 = records.iter().map(|r| r.qty).sum();
        // Span in days between earliest and latest usage (>=1).
        let min_d = records.iter().map(|r| r.date).min().unwrap();
        let max_d = records.iter().map(|r| r.date).max().unwrap();
        let span = (max_d - min_d).num_days().max(1) as f64;
        let daily = total / span;
        let lead = part.lead_time_days as f64;
        let lead_demand = daily * lead;
        // Safety stock ~ sqrt(lead) * daily (simple heuristic).
        let safety = daily * lead.sqrt();
        let recommended_rop = (lead_demand + safety).ceil();
        let available = self.available_total(part_id);
        let days_cover = if daily > 0.0 { available / daily } else { f64::INFINITY };
        Some(serde_json::json!({
            "part_id": part_id,
            "part_number": part.part_number,
            "usage_records": records.len(),
            "avg_daily_usage": (daily*1000.0).round()/1000.0,
            "lead_time_days": part.lead_time_days,
            "projected_demand_over_horizon": ((daily * horizon_days as f64)*1000.0).round()/1000.0,
            "horizon_days": horizon_days,
            "recommended_reorder_point": recommended_rop,
            "available_now": available,
            "days_of_cover": if days_cover.is_finite() { serde_json::json!((days_cover*10.0).round()/10.0) } else { serde_json::Value::Null },
            "stockout_risk": days_cover < lead,
        }))
    }

    pub fn audit_log(&self, limit: usize) -> Vec<AuditEntry> {
        let log = self.audit_log.lock().unwrap();
        log.iter().rev().take(limit).cloned().collect()
    }

    // ─── seed ────────────────────────────────────────────────────────────

    fn seed(&self) {
        let today = Utc::now().date_naive();

        let acme = self.create_supplier("Acme Parts Co", 14, "system");
        let _fast = self.create_supplier("FastSpares Ltd", 5, "system");

        // Locations.
        let wh = self.create_location("Central Warehouse", "warehouse", "system");
        let van = self.create_location("Service Van 7", "van", "system");

        // Parts: a brake assembly (BOM parent), pads + rotor (children), an oil filter (consumable), and a superseded sensor.
        let brake = self.create_part("BRK-ASSY-100", "Brake assembly", "brakes", "ea", Some(acme.id.clone()), 14, 120.0, "system");
        let pads = self.create_part("BRK-PAD-22", "Brake pad set", "brakes", "set", Some(acme.id.clone()), 7, 35.0, "system");
        let rotor = self.create_part("BRK-ROT-9", "Brake rotor", "brakes", "ea", Some(acme.id.clone()), 10, 60.0, "system");
        let filter = self.create_part("OIL-FLT-3", "Oil filter", "filters", "ea", Some(acme.id.clone()), 7, 8.0, "system");
        let sensor_old = self.create_part("SEN-O2-1", "O2 sensor (rev A)", "sensors", "ea", Some(acme.id.clone()), 21, 45.0, "system");
        let sensor_new = self.create_part("SEN-O2-2", "O2 sensor (rev B)", "sensors", "ea", Some(acme.id.clone()), 14, 48.0, "system");

        // BOM: brake assembly = 1 rotor + 2 pad sets.
        self.add_bom_line(&brake.id, &rotor.id, 1.0, "system").ok();
        self.add_bom_line(&brake.id, &pads.id, 2.0, "system").ok();

        // Supersession: old O2 sensor -> new.
        self.add_supersession(&sensor_old.id, &sensor_new.id, "rev A discontinued", "system").ok();

        // Stock + policies.
        self.set_stock_policy(&pads.id, &wh.id, 20.0, 40.0, "system").ok();
        self.receive_stock(&pads.id, &wh.id, 50.0, "initial", "system").ok();
        self.set_stock_policy(&rotor.id, &wh.id, 10.0, 20.0, "system").ok();
        self.receive_stock(&rotor.id, &wh.id, 8.0, "initial", "system").ok(); // below ROP -> reorder
        self.set_stock_policy(&filter.id, &wh.id, 30.0, 100.0, "system").ok();
        self.receive_stock(&filter.id, &wh.id, 200.0, "initial", "system").ok();
        self.receive_stock(&filter.id, &van.id, 12.0, "initial", "system").ok();
        self.receive_stock(&sensor_new.id, &wh.id, 15.0, "initial", "system").ok();

        // Usage history for the oil filter (steady consumption -> forecastable).
        for k in 0..12 {
            let date = today - Duration::days(60 - k * 5);
            self.usage.lock().unwrap().push(UsageRecord { part_id: filter.id.clone(), date, qty: 6.0 + (k % 3) as f64 });
        }
    }
}
