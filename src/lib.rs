//! Parts MCP Server library surface.
//!
//! A parts & spares platform: catalog, multi-location inventory with reorder
//! policy, BOM & where-used, supersession chains, suppliers, stock movements
//! (receipt/issue/transfer/adjust), reservations, reorder suggestions, and
//! usage-based demand forecasting — over a stock-movement audit trail.

pub mod server;
pub mod store;
pub mod types;
