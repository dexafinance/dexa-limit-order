use crate::state::{remove_order, store_new_order, Config, OrderInfo, CONFIG, ORDERS};
use cosmwasm_std::{
    attr, to_binary, Coin, CosmosMsg, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
    Uint128, WasmMsg,
};
use cw20::Cw20ExecuteMsg;
//PairInfo
use terraswap::asset::{Asset, AssetInfo};
use terraswap::pair::{
    Cw20HookMsg as PairCw20HookMsg, ExecuteMsg as PairExecuteMsg, SimulationResponse,
};
//query_pair_info
use terraswap::querier::{simulate};

pub fn submit_order(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    pair_addr: String,
    offer_asset: Asset,
    ask_asset: Asset,
    fee_amount: Uint128,
) -> StdResult<Response> {
    let config: Config = CONFIG.load(deps.storage)?;

    if fee_amount < config.min_fee_amount {
        return Err(StdError::generic_err(format!(
            "fee should be greater than {}",
            config.min_fee_amount
        )));
    }

    let fee_included = offer_asset.info == config.fee_token;

    let new_offer_asset = if fee_included {
        let amount = offer_asset.amount + fee_amount;
        Asset {
            amount,
            ..offer_asset.clone()
        }
    } else {
        offer_asset.clone()
    };


    // check if the pair exists
    // ignore this check for now, use the pair_addr from parameter instead
    // let pair_info: PairInfo = query_pair_info(
    //     &deps.querier,
    //     config.terraswap_factory,
    //     &[offer_asset.info.clone(), ask_asset.info.clone()],
    // )
    // .map_err(|_| StdError::generic_err("there is no terraswap pair for the 2 assets provided"))?;

    let mut messages: Vec<CosmosMsg> = vec![];

    match new_offer_asset.info.clone() {
        AssetInfo::NativeToken { .. } => new_offer_asset.assert_sent_native_token_balance(&info)?,
        AssetInfo::Token { contract_addr } => {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::TransferFrom {
                    owner: info.sender.to_string(),
                    recipient: env.contract.address.to_string(),
                    amount: new_offer_asset.amount,
                })?,
            }));
        }
    }

    // transfer fee to self
    if !fee_included {
        match config.fee_token.clone() {
            AssetInfo::NativeToken { .. } => {
                Asset { amount: fee_amount, info : config.fee_token.clone()}.assert_sent_native_token_balance(&info)?
            },
            AssetInfo::Token { contract_addr } => {
                messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr,
                    funds: vec![],
                    msg: to_binary(&Cw20ExecuteMsg::TransferFrom {
                        owner: info.sender.to_string(),
                        recipient: env.contract.address.to_string(),
                        amount: fee_amount,
                    })?,
                }));
            }
        }
    }

    let mut new_order = OrderInfo {
        order_id: 0u64, // provisional
        bidder_addr: deps.api.addr_validate(info.sender.as_str())?,
        pair_addr: deps.api.addr_validate(&pair_addr)?,
        offer_asset: offer_asset.clone(),
        ask_asset: ask_asset.clone(),
        fee_amount,
    };
    store_new_order(deps.storage, &mut new_order)?;

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "submit_order"),
        attr("order_id", new_order.order_id.to_string()),
        attr("bidder_addr", info.sender.to_string()),
        attr("offer_asset", offer_asset.to_string()),
        attr("ask_asset", ask_asset.to_string()),
    ]))
}

pub fn cancel_order(deps: DepsMut, info: MessageInfo, order_id: u64) -> StdResult<Response> {
    let config: Config = CONFIG.load(deps.storage)?;
    let order: OrderInfo = ORDERS.load(deps.storage, &order_id.to_be_bytes())?;
    if order.bidder_addr != info.sender {
        return Err(StdError::generic_err("unauthorized"));
    }

    // refund offer asset
    let mut messages: Vec<CosmosMsg> = vec![order
        .offer_asset
        .clone()
        .into_msg(&deps.querier, order.bidder_addr.clone())?];

    // refund fee
    let refund_fee_asset = Asset {
        info: config.fee_token.clone(),
        amount: order.fee_amount,
    };
    messages.push(
        refund_fee_asset
            .clone()
            .into_msg(&deps.querier, order.bidder_addr.clone())?,
    );

    remove_order(deps.storage, &order);

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "cancel_order"),
        attr("order_id", order_id.to_string()),
        attr("refunded_asset", order.offer_asset.to_string()),
        attr("refunded_fee", refund_fee_asset.to_string()),
    ]))
}

pub fn execute_order(deps: DepsMut, _info: MessageInfo, order_id: u64) -> StdResult<Response> {
    let config: Config = CONFIG.load(deps.storage)?;
    let order: OrderInfo = ORDERS.load(deps.storage, &order_id.to_be_bytes())?;

    // deduct tax if native
    let offer_asset = if order.offer_asset.is_native_token() {
        let amount = order.offer_asset.deduct_tax(&deps.querier)?.amount;

        Asset {
            amount,
            ..order.offer_asset.clone()
        }
    } else {
        order.offer_asset.clone()
    };

    let simul_res: SimulationResponse =
        simulate(&deps.querier, order.pair_addr.clone(), &offer_asset)?;
    if simul_res.return_amount < order.ask_asset.amount {
        return Err(StdError::generic_err("insufficient return amount"));
    }

    let mut messages: Vec<CosmosMsg> = vec![];

    // create swap message
    match offer_asset.clone().info {
        AssetInfo::Token { contract_addr } => {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::Send {
                    contract: order.pair_addr.to_string(),
                    amount: offer_asset.amount,
                    msg: to_binary(&PairCw20HookMsg::Swap {
                        to: None,
                        belief_price: None,
                        max_spread: None,
                    })?,
                })?,
            }));
        }
        AssetInfo::NativeToken { denom } => {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: order.pair_addr.to_string(),
                funds: vec![Coin {
                    denom,
                    amount: offer_asset.amount,
                }],
                msg: to_binary(&PairExecuteMsg::Swap {
                    offer_asset,
                    belief_price: None,
                    max_spread: None,
                    to: None,
                })?,
            }));
        }
    };

    // send asset to bidder
    messages.push(
        order
            .ask_asset
            .clone()
            .into_msg(&deps.querier, order.bidder_addr.clone())?,
    );

    // executor might earn config.executor_fee_percent amount of excess and fee, but 
    // for simplicity it is disabled now. Will explore this option later.

    // send excess amount to reserve
    let excess_amount: Uint128 = simul_res.return_amount - order.ask_asset.amount;
    if excess_amount > Uint128::zero() {
        let excess_asset = Asset {
            amount: excess_amount,
            info: order.ask_asset.info.clone(),
        };
        messages.push(excess_asset.into_msg(&deps.querier, deps.api.addr_validate(&config.reserve_addr)?)?);
    }

    // send fee to reserve
    let fee_asset = Asset {
        amount: order.fee_amount,
        info: config.fee_token.clone()
    };
    messages.push(fee_asset.clone().into_msg(&deps.querier, deps.api.addr_validate(&config.reserve_addr)?)?);

    remove_order(deps.storage, &order);

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "execute_order"),
        attr("order_id", order.order_id.to_string()),
        attr("fee_amount", fee_asset.amount.to_string()),
        attr("excess_amount", excess_amount.to_string()),
    ]))
}
