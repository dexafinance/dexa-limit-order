use crate::state::{remove_order, store_new_order, Config, OrderInfo, RecurringOrderOpt, CONFIG, ORDERS, POOL_PRISM};
use cosmwasm_bignumber::{Decimal256};
use cosmwasm_std::{
    attr, to_binary, Coin, CosmosMsg, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
    Uint128, Decimal, WasmMsg, QuerierWrapper, Addr
};
use cw20::Cw20ExecuteMsg;
use std::str::FromStr;
//PairInfo
use terraswap::asset::{Asset, AssetInfo};
use terraswap::pair::{
    Cw20HookMsg as PairCw20HookMsg, ExecuteMsg as PairExecuteMsg, SimulationResponse,
};
use cw_asset::{Asset as CwAsset, AssetInfo as CwAssetInfo};

//query_pair_info
use terraswap::querier::{simulate};
use prismswap::querier::{simulate as simulate_prism};
use prismswap::pair::{ExecuteMsg as PrismPairExecuteMsg, SimulationResponse as PrismSimulationResponse};

pub fn submit_order(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    pair_addr: String,
    offer_asset: Asset,
    ask_asset: Asset,
    fee_amount: Uint128,
    recurring: Option<RecurringOrderOpt>,
) -> StdResult<Response> {
    let config: Config = CONFIG.load(deps.storage)?;

    if fee_amount < config.min_fee_amount {
        return Err(StdError::generic_err(format!(
            "fee should be greater than {}",
            config.min_fee_amount
        )));
    }

    // fee_included meaning fee token is the same with offer_asset
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
    if !fee_included && fee_amount > Uint128::zero() {
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
        recurring: recurring.clone()
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

fn simulate_prism_adapter(querier: &QuerierWrapper,
    pair_contract: &Addr,
    offer_asset: &Asset) -> StdResult<SimulationResponse> {
    let prism_offer_asset = CwAsset {
            amount: offer_asset.amount,
            info: match &offer_asset.info {
                AssetInfo::NativeToken { denom } => CwAssetInfo::Native(denom.clone()),
                AssetInfo::Token { contract_addr } => CwAssetInfo::Cw20(Addr::unchecked(contract_addr.clone()))
            }
    };

    // SimulationResponse is the same between terraswap and prismswap
    let simul_res: PrismSimulationResponse =
        simulate_prism(querier, pair_contract, &prism_offer_asset)?;

    Ok(SimulationResponse {
        return_amount: simul_res.return_amount,
        spread_amount: simul_res.spread_amount,
        commission_amount: simul_res.commission_amount
    })
}

fn simulate_multipools(querier: &QuerierWrapper,
    dex: String,
    pair_contract: Addr,
    offer_asset: &Asset) -> StdResult<SimulationResponse> {
    if dex == POOL_PRISM {
        simulate_prism_adapter(querier, &pair_contract, offer_asset)
    } else {
        // astroport and terraswap share the same interface
        simulate(querier, pair_contract, offer_asset)
    }
}

pub fn execute_order(deps: DepsMut, _env: Env, info: MessageInfo, order_id: u64, dex: String) -> StdResult<Response> {
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
        simulate_multipools(&deps.querier, dex.clone(), order.pair_addr.clone(), &offer_asset)?;

    if simul_res.return_amount < order.ask_asset.amount {
        return Err(StdError::generic_err("insufficient return amount"));
    }

    let mut messages: Vec<CosmosMsg> = vec![];

    // create swap message
    // fix bug swap on astroport bLUNA-LUNA return spread larger than 0.5% causing transaction to fail eventhough
    // actually less than 0.5% spread from belief_price
    let belief_price: Option<Decimal> = Some(Decimal::from_ratio(offer_asset.amount, order.ask_asset.amount));
    // default to max_spread 0.5%
    // as of 2022/05/04 astroport apply default 0.5% max_spread while terraswap have none i.e. will not check spread if passing none
    let max_spread: Option<Decimal> = Some(Decimal::from_str("0.005")?);
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
                        belief_price: belief_price,
                        max_spread: max_spread,
                    })?,
                })?,
            }));
        }
        AssetInfo::NativeToken { denom } => {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: order.pair_addr.to_string(),
                funds: vec![Coin {
                    denom: denom.clone(),
                    amount: offer_asset.amount,
                }],
                msg: if dex == POOL_PRISM {
                    to_binary(&PrismPairExecuteMsg::Swap {
                        offer_asset: 
                            CwAsset {
                                amount: offer_asset.amount,
                                info: CwAssetInfo::Native(denom.clone())
                            },
                        belief_price: belief_price,
                        max_spread: max_spread,
                        to: None,
                    })?
                } else {
                    to_binary(&PairExecuteMsg::Swap {
                        offer_asset,
                        belief_price: belief_price,
                        max_spread: max_spread,
                        to: None,
                    })?
                },
            }));
        }
    };

    // keep asset for new order if the order is recurring
    // let mut remaining_loop = 0;
    let mut is_last_order = true;
    let mut fee_amount = order.fee_amount;
    if let Some(recurring) = order.recurring.clone() {
        // remaining_loop = recurring.remaining_loop
        if recurring.remaining_loop > 0 {
            // fee for current order execution, fee left = current order.fee_amount - fee_amount
            fee_amount = order.fee_amount * Decimal::from_ratio(Uint128::from(1u64), Uint128::from(recurring.remaining_loop + 1));

            is_last_order = false;
        }
    }

    // else send asset to bidder
    if is_last_order {
        messages.push(
            order
                .ask_asset
                .clone()
                .into_msg(&deps.querier, order.bidder_addr.clone())?,
        );
    }

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

    // send fee to reserve, take a portion of fee equivalent to number of loop
    if !fee_amount.is_zero() {
        let fee_asset = Asset {
            amount: fee_amount,
            info: config.fee_token.clone()
        };
        messages.push(fee_asset.clone().into_msg(&deps.querier, deps.api.addr_validate(&config.reserve_addr)?)?);
    }

    remove_order(deps.storage, &order);

    if !is_last_order {
        // reverse offer_asset and ask_asset
        // belief_price = offer_asset.amount / ask_asset.amount
        // case 1: start with 100 LUNA, sell at 90$ and buy back at 85$ and repeat
        // input: offer_asset { 100 LUNA } ->  ask_asset { 9000 UST } belief_price 100/9000 = 0.011111 swapback_belief_price 85.0
        // execute order 1st: 100 LUNA -> 9000 UST as inputted
        // execute order 2nd: 9000 UST -> 9000*1/85.0 = 105.88 LUNA (*1/swapback_belief_price)
        // execute order 3rd: 105.88 LUNA -> 105.88*(1/0.011111) UST (*1/belief_price)

        // case 2: start with 8500 UST, buy 100 LUNA at 85$ and sell back at 90$ and repeat
        // input: offer_asset { 8500 UST } -> ask_asset { 100 LUNA } belief_price 85.0 swapback_belief_price 1/90.0 = 0.011111
        // execute order 1st: 8500 UST -> 100 LUNA as inputted 
        // execute order 2nd: 100 LUNA -> 100*(1/0.011111) = 9000 UST (*1/swapback_belief_price)
        // on next swap 9000 UST -> 9000*1/85.0 = 105.88 LUNA (*1/belief_price)

        let new_offer_asset = order.ask_asset.clone();
        let recurring = order.recurring.unwrap();

        let amount = if (recurring.total_loop - recurring.remaining_loop) % 2 == 0 {
            // after 1st, 3rd, 5th ... execution
            new_offer_asset.amount * Decimal::from(Decimal256::one() / Decimal256::from(recurring.swapback_belief_price))
        } else {
            // after 2nd, 4th, 6th ... execution
            new_offer_asset.amount * Decimal::from(Decimal256::one() / Decimal256::from(recurring.belief_price))
        };

        let new_ask_asset = Asset {
            amount,    
            ..order.offer_asset.clone()
        };

        let mut new_order = OrderInfo {
            order_id: 0u64, // provisional
            bidder_addr: deps.api.addr_validate(info.sender.as_str())?,
            pair_addr: order.pair_addr,
            offer_asset: new_offer_asset,
            ask_asset: new_ask_asset,
            fee_amount: if is_last_order { order.fee_amount } else { order.fee_amount - fee_amount },
            recurring: Some(RecurringOrderOpt {
                    remaining_loop: recurring.remaining_loop - 1,
                    ..recurring
                })
        };
        store_new_order(deps.storage, &mut new_order)?;
    };

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "execute_order"),
        attr("order_id", order.order_id.to_string()),
        attr("fee_amount", fee_amount.to_string()),
        attr("excess_amount", excess_amount.to_string()),
    ]))
}
