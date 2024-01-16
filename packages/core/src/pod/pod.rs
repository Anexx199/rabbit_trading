use std::collections::LinkedList;
use tokio::sync::RwLockReadGuard;

use super::event::{event_bus::EventBus, listener::initializer::get_event_listener};
use crate::{
    broker::{common::broker::BrokerTrait, initializer::get_broker_instance},
    metrics::initializer::get_metrics_registry_factory,
    model::{common::error::Error, config::pod::PodConfig, trading::event::RabbitTradingEvent},
    persistent_kv::{
        common::store::PersistentKVStoreTrait, initializer::get_persistent_kv_instance,
    },
    pod::interceptor::factory::PodBrokerInterceptorCollectionFactory,
    strategy::{
        common::strategy::{StrategyContext, StrategyTrait},
        initializer::get_strategy_instance,
    },
};

pub struct Pod {
    pod_config: PodConfig,
    event_bus: EventBus,
}

impl Pod {
    pub fn new(pod_config: PodConfig, pod_id: String) -> Self {
        const EMPTY_BROKER_ID: &'static str = "";

        Pod {
            pod_config,
            event_bus: EventBus::new(EMPTY_BROKER_ID.to_owned(), pod_id),
        }
    }

    fn initialize_broker_list(&self) -> Result<Vec<Box<dyn BrokerTrait>>, Error> {
        const ILLEGAL_BROKER_ID: &'static str = "ILLEGAL_BROKER_ID";

        let broker_list: Vec<Box<dyn BrokerTrait>> = self
            .pod_config
            .broker_list
            .iter()
            .filter_map(|broker_config| {
                let metrics_registry_factory = get_metrics_registry_factory(
                    self.pod_config.metrics_registry.identifier.clone(),
                    self.pod_config.metrics_registry.config_map.clone(),
                )
                .ok()?;

                get_broker_instance(
                    broker_config.identifier.clone(),
                    Box::new(PodBrokerInterceptorCollectionFactory::new(
                        self.event_bus
                            .shallow_clone(Option::Some(broker_config.identifier.clone())),
                        metrics_registry_factory,
                    )),
                    broker_config.config_map.clone(),
                )
                .ok()
            })
            .collect();

        if broker_list.len() != self.pod_config.broker_list.len() {
            let broker_id_list: Vec<String> = self
                .pod_config
                .broker_list
                .iter()
                .map(|broker_config| broker_config.identifier.clone())
                .collect();

            return Result::Err(Error {
                code: ILLEGAL_BROKER_ID.to_owned(),
                message: broker_id_list.join(","),
            });
        }

        Result::Ok(broker_list)
    }

    async fn initialize_persistent_kv_store(
        &self,
    ) -> Result<Box<dyn PersistentKVStoreTrait>, Error> {
        get_persistent_kv_instance(
            self.pod_config.persistent_kv_store.identifier.clone(),
            self.pod_config.persistent_kv_store.config_map.clone(),
        )
        .await
    }

    fn initialize_strategy(
        &self,
        broker_list: Vec<Box<dyn BrokerTrait>>,
        persistent_kv_store: Box<dyn PersistentKVStoreTrait>,
    ) -> Result<Box<dyn StrategyTrait>, Error> {
        get_strategy_instance(
            self.pod_config.strategy.identifier.clone(),
            StrategyContext {
                broker_list,
                persistent_kv_store,
                config_map: self.pod_config.strategy.config_map.clone(),
            },
        )
    }

    fn initialize_event_listeners(&self) -> Result<(), Error> {
        match self
            .pod_config
            .event_listener_list
            .iter()
            .find_map(|event_listener_config| {
                get_event_listener(
                    event_listener_config.identifier.clone(),
                    event_listener_config.config_map.clone(),
                )
                .map(|event_listener| event_listener.start(self.event_bus.subscribe()))
                .err()
            }) {
            None => Result::Ok(()),
            Some(err) => Result::Err(err),
        }
    }

    async fn initialize(&self) -> Result<Box<dyn StrategyTrait>, Error> {
        let broker_list = self.initialize_broker_list()?;
        for broker in broker_list.iter() {
            if let Some(heartbeat) = broker.create_heartbeat().await {
                heartbeat.start().await?;
                // todo: manager the lifecycle of heartbeat (stop)
            }
        }
        let persistent_kv_store = self.initialize_persistent_kv_store().await?;
        self.initialize_event_listeners()?;
        self.initialize_strategy(broker_list, persistent_kv_store)
    }

    pub async fn inspect_log(&self) -> RwLockReadGuard<'_, LinkedList<RabbitTradingEvent>> {
        self.event_bus.inspect_log().await
    }

    pub async fn start(&self) -> Result<(), Error> {
        let strategy_instance = self.initialize().await?;
        strategy_instance.start().await
    }

    pub async fn stop(&self) -> Result<(), Error> {
        todo!() // gracefully exit
    }
}
