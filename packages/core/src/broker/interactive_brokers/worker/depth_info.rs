use anyhow::{anyhow, Error};
use async_trait::async_trait;
use ibkr_client_portal::model::{
    definition::TickType,
    streaming::{
        StreamingDataResponse, SubscribeMarketDataRequest, ToStructuredRequest,
        UnsubscribeMarketDataRequest,
    },
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc::Sender;

use crate::{
    broker::{
        common::subscription::{SubscriptionController, SubscriptionWorker},
        interactive_brokers::broker::InteractiveBrokersBroker,
    },
    model::{
        common::types::ConfigMap,
        trading::{
            quote::{Depth, QuoteDepthInfo},
            symbol::Symbol,
        },
    },
    utils::time::get_now_unix_timestamp,
};

pub struct IBQuoteDepthInfoSubscriptionWorker {
    config_map: ConfigMap,
    symbol: Symbol,
    sys_sender: Sender<QuoteDepthInfo>,
    local_stopped_indicator: Arc<AtomicBool>,
    global_stopped_indicator: Arc<AtomicBool>,
}

impl IBQuoteDepthInfoSubscriptionWorker {
    pub fn new(
        config_map: ConfigMap,
        symbol: Symbol,
        sys_sender: Sender<QuoteDepthInfo>,
        local_stopped_indicator: Arc<AtomicBool>,
        global_stopped_indicator: Arc<AtomicBool>,
    ) -> Self {
        IBQuoteDepthInfoSubscriptionWorker {
            config_map,
            symbol,
            sys_sender,
            local_stopped_indicator,
            global_stopped_indicator,
        }
    }
}

#[async_trait]
impl SubscriptionWorker for IBQuoteDepthInfoSubscriptionWorker {
    async fn start(mut self) -> Result<(), Error> {
        let conid = InteractiveBrokersBroker::get_conid_from_symbol(&self.symbol).await;
        let client_portal =
            InteractiveBrokersBroker::create_ib_client_portal(self.config_map.clone());
        let (sender, receiver) = client_portal.connect_to_websocket().await.unwrap();
        if let Err(err) = sender
            .send_streaming_structured_data_request(
                SubscribeMarketDataRequest {
                    conid: format!("{}", conid),
                    fields: vec![
                        TickType::AskPrice,
                        TickType::AskSize,
                        TickType::BidPrice,
                        TickType::BidSize,
                    ]
                    .into_iter()
                    .map(|field| field.to_string())
                    .collect(),
                }
                .to_structured_request(),
            )
            .await
        {
            return Result::Err(anyhow!("Error when subscribing market data {:?}", err));
        }

        loop {
            if self.global_stopped_indicator.load(Ordering::Relaxed)
                || self.local_stopped_indicator.load(Ordering::Relaxed)
            {
                if let Err(err) = sender
                    .send_streaming_structured_data_request(
                        UnsubscribeMarketDataRequest {
                            conid: format!("{}", conid),
                        }
                        .to_structured_request(),
                    )
                    .await
                {
                    return Result::Err(anyhow!("Error when closing streaming {:?}", err));
                }
                return Result::Ok(());
            }

            match receiver.receive().await {
                Ok(streaming_data) => match streaming_data {
                    StreamingDataResponse::MarketData(data) => {
                        // TODO: use the macro to unify the codes
                        let timestamp = get_now_unix_timestamp();
                        let ask_depth = Depth {
                            position: Option::None,
                            price: data.ask_price.unwrap(),
                            volume: data.ask_size.unwrap(),
                            order_count: Option::None,
                        };
                        let bid_depth = Depth {
                            position: Option::None,
                            price: data.bid_price.unwrap(),
                            volume: data.bid_size.unwrap(),
                            order_count: Option::None,
                        };
                        let quote_depth_info = QuoteDepthInfo {
                            symbol: self.symbol.clone(),
                            sequence: timestamp,
                            timestamp,
                            ask_list: vec![ask_depth],
                            bid_list: vec![bid_depth],
                        };

                        if let Err(send_err) = self.sys_sender.send(quote_depth_info).await {
                            log::warn!("Error when sending message {:?}", send_err);
                        }
                    }
                    _ => continue,
                },
                Err(streaming_err) => {
                    log::warn!("Streaming Error {:?}", streaming_err);
                }
            }
        }
    }
}

pub struct IBQuoteDepthInfoSubscriptionController {
    local_stopped_indicator: Arc<AtomicBool>,
}

impl IBQuoteDepthInfoSubscriptionController {
    pub fn new(local_stopped_indicator: Arc<AtomicBool>) -> Self {
        IBQuoteDepthInfoSubscriptionController {
            local_stopped_indicator,
        }
    }
}

#[async_trait]
impl SubscriptionController for IBQuoteDepthInfoSubscriptionController {
    async fn stop(self) -> Result<(), Error> {
        self.local_stopped_indicator.store(false, Ordering::Relaxed);
        Result::Ok(())
    }
}
