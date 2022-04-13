use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::Uint128;
use terraswap::asset::{Asset, AssetInfo};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub fee_token: AssetInfo,
    pub min_fee_amount: Uint128,
    // 1000 = 1000/1000000=0.1%
    pub min_fee_percent: Uint128,
    pub executor_fee_percent: Uint128,
    pub reserve_addr: String
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    // update config, executor must be reserver_addr for security
    UpdateConfig {  
        fee_token: AssetInfo,
        min_fee_amount: Uint128,
        // 1000 = 1000/1000000=0.1%
        min_fee_percent: Uint128,
        executor_fee_percent: Uint128,
        reserve_addr: String
    },
    /// User submits a new order
    /// Before, the user should increase allowance for the offer_asset (or send the native token) and the fee
    SubmitOrder {
        pair_addr: String,
        offer_asset: Asset,
        ask_asset: Asset,
        fee_amount: Uint128,
    },
    /// User operation to canel an existing order
    CancelOrder { order_id: u64 },
    /// Executor operation to execute an existing order
    ExecuteOrder { order_id: u64 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    Order {
        order_id: u64,
    },
    Orders {
        bidder_addr: Option<String>,
        start_after: Option<u64>,
        limit: Option<u32>,
        order_by: Option<OrderBy>,
    },
    LastOrderId {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
    pub fee_token: AssetInfo,
    pub min_fee_amount: Uint128,
    pub min_fee_percent: Uint128,
    pub executor_fee_percent: Uint128,
    pub reserve_addr: String
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct OrderResponse {
    pub order_id: u64,
    pub bidder_addr: String,
    pub pair_addr: String,
    pub offer_asset: Asset,
    pub ask_asset: Asset,
    pub fee_amount: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct OrdersResponse {
    pub orders: Vec<OrderResponse>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LastOrderIdResponse {
    pub last_order_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderBy {
    Asc,
    Desc,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsg {}
