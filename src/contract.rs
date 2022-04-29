#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};

use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::order::{cancel_order, execute_order, submit_order};
use crate::query::{query_config, query_last_order_id, query_order, query_orders};
use crate::state::{Config, CONFIG, LAST_ORDER_ID};

use cosmwasm_std::{Uint128, StdError};
use terraswap::asset::{AssetInfo};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {

    LAST_ORDER_ID.save(deps.storage, &0u64)?;

    update_config(deps,
        _info,
        true,
        msg.fee_token,
        msg.min_fee_amount,
        msg.min_fee_percent,
        msg.executor_fee_percent,
        msg.reserve_addr
    )?;

    Ok(Response::default())
}

pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    init: bool,
    fee_token: AssetInfo,
    min_fee_amount: Uint128, 
    min_fee_percent: Uint128, 
    executor_fee_percent: Uint128, 
    reserve_addr: String
) -> StdResult<Response> {

    if !init {
        // only allow to change config if executor is reserve_addr
        let config: Config = CONFIG.load(deps.storage)?;
        if info.sender.to_string() != config.reserve_addr {
            return Err(StdError::generic_err("unauthorized, only reserve_addr owner can change config"));
        }
    }

    let config = Config {
        fee_token: fee_token,
        min_fee_amount: min_fee_amount,
        min_fee_percent: min_fee_percent,
        executor_fee_percent: executor_fee_percent,
        reserve_addr: reserve_addr
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> StdResult<Response> {
    match msg {
        ExecuteMsg::UpdateConfig {
            fee_token,
            min_fee_amount,
            min_fee_percent,
            executor_fee_percent,
            reserve_addr
        } => update_config(deps, info, false, fee_token, min_fee_amount, min_fee_percent, executor_fee_percent, reserve_addr),
        ExecuteMsg::SubmitOrder {
            pair_addr,
            offer_asset,
            ask_asset,
            fee_amount,
        } => submit_order(deps, env, info, pair_addr, offer_asset, ask_asset, fee_amount),
        ExecuteMsg::CancelOrder { order_id } => cancel_order(deps, info, order_id),
        ExecuteMsg::ExecuteOrder { order_id, dex } => execute_order(deps, info, order_id, dex),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::Order { order_id } => to_binary(&query_order(deps, order_id)?),
        QueryMsg::Orders {
            bidder_addr,
            start_after,
            limit,
            order_by,
        } => to_binary(&query_orders(
            deps,
            bidder_addr,
            start_after,
            limit,
            order_by,
        )?),
        QueryMsg::LastOrderId {} => to_binary(&query_last_order_id(deps)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> StdResult<Response> {
    Ok(Response::default())
}
