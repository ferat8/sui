// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fmt::Write;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use anyhow::anyhow;
use futures::StreamExt;
use futures_core::Stream;
use jsonrpsee::core::client::Subscription;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use move_binary_format::access::ModuleAccess;
use move_bytecode_utils::module_cache::SyncModuleCache;
use move_core_types::identifier::Identifier;
use move_core_types::language_storage::{StructTag, TypeTag};
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;

// re-export essential sui crates
pub use sui_config::gateway;
use sui_config::gateway::GatewayConfig;
use sui_core::gateway_state::{GatewayClient, GatewayState};
pub use sui_json as json;
use sui_json_rpc::api::EventStreamingApiClient;
use sui_json_rpc::api::RpcBcsApiClient;
use sui_json_rpc::api::RpcFullNodeReadApiClient;
use sui_json_rpc::api::RpcGatewayApiClient;
use sui_json_rpc::api::RpcReadApiClient;
use sui_json_rpc::api::WalletSyncApiClient;
pub use sui_json_rpc_types as rpc_types;
use sui_json_rpc_types::{
    GatewayTxSeqNumber, GetObjectDataResponse, GetRawObjectDataResponse, SuiEventEnvelope,
    SuiEventFilter, SuiObjectInfo, SuiParsedObject, SuiTransactionResponse,
};
pub use sui_types as types;
use sui_types::base_types::{ObjectID, SuiAddress, TransactionDigest};
use sui_types::crypto::SuiSignature;
use sui_types::messages::Transaction;
use sui_types::object::{Data, Object, ObjectFormatOptions};
use sui_types::sui_serde::Base64;

use crate::client_state::{ClientState, ResolverWrapper};
use crate::transaction_builder::TransactionBuilder;

mod client_state;
pub mod crypto;
mod transaction_builder;

pub struct SuiClient {
    api: Arc<SuiClientApi>,
    transaction_builder: TransactionBuilder,
    read_api: Arc<ReadApi>,
    full_node_api: FullNodeApi,
    event_api: EventApi,
    quorum_driver: QuorumDriver,
}

#[allow(clippy::large_enum_variant)]
enum SuiClientApi {
    Rpc(HttpClient, Option<WsClient>),
    Embedded(GatewayClient),
}

impl SuiClient {
    pub async fn new_rpc_client(
        http_url: &str,
        ws_url: Option<&str>,
    ) -> Result<SuiClient, anyhow::Error> {
        let client = HttpClientBuilder::default().build(http_url)?;

        let ws_client = if let Some(url) = ws_url {
            Some(WsClientBuilder::default().build(url).await?)
        } else {
            None
        };
        Ok(SuiClient::new(SuiClientApi::Rpc(client, ws_client)))
    }

    pub fn new_embedded_client(config: &GatewayConfig) -> Result<SuiClient, anyhow::Error> {
        let state = GatewayState::create_client(config, None)?;
        Ok(SuiClient::new(SuiClientApi::Embedded(state)))
    }
    fn new(api: SuiClientApi) -> Self {
        let api = Arc::new(api);
        let state = Arc::new(RwLock::new(ClientState::default()));
        let read_api = Arc::new(ReadApi {
            api: api.clone(),
            state: state.clone(),
            module_cache: SyncModuleCache::new(ResolverWrapper(state.clone())),
        });
        let full_node_api = FullNodeApi(api.clone());
        let event_api = EventApi(api.clone());
        let transaction_builder = TransactionBuilder {
            read_api: read_api.clone(),
        };
        let quorum_driver = QuorumDriver {
            api: api.clone(),
            state,
        };
        SuiClient {
            api,
            transaction_builder,
            read_api,
            full_node_api,
            event_api,
            quorum_driver,
        }
    }
}

pub struct ReadApi {
    api: Arc<SuiClientApi>,
    state: Arc<RwLock<ClientState>>,
    module_cache: SyncModuleCache<ResolverWrapper<ClientState>>,
}

impl ReadApi {
    pub async fn get_objects_owned_by_address(
        &self,
        address: SuiAddress,
    ) -> anyhow::Result<Vec<SuiObjectInfo>> {
        Ok(match &*self.api {
            SuiClientApi::Rpc(c, _) => c.get_objects_owned_by_address(address).await?,
            SuiClientApi::Embedded(c) => c.get_objects_owned_by_address(address).await?,
        })
    }

    pub async fn get_objects_owned_by_object(
        &self,
        object_id: ObjectID,
    ) -> anyhow::Result<Vec<SuiObjectInfo>> {
        Ok(match &*self.api {
            SuiClientApi::Rpc(c, _) => c.get_objects_owned_by_object(object_id).await?,
            SuiClientApi::Embedded(c) => c.get_objects_owned_by_object(object_id).await?,
        })
    }

    pub async fn get_parsed_object(
        &self,
        object_id: ObjectID,
    ) -> anyhow::Result<GetObjectDataResponse> {
        let response = self.get_object(object_id).await?;
        self.parse_object_response(response).await
    }

    pub async fn get_object(
        &self,
        object_id: ObjectID,
    ) -> anyhow::Result<GetRawObjectDataResponse> {
        let response = self.state.read().await.get_object(object_id).cloned();
        Ok(if let Some(response) = response {
            response
        } else {
            let response = match &*self.api {
                SuiClientApi::Rpc(c, _) => c.get_raw_object(object_id).await?,
                SuiClientApi::Embedded(c) => c.get_raw_object(object_id).await?,
            };
            self.state.write().await.update_object(response.clone());
            response
        })
    }

    async fn parse_object_response(
        &self,
        object: GetRawObjectDataResponse,
    ) -> Result<GetObjectDataResponse, anyhow::Error> {
        Ok(match object {
            GetRawObjectDataResponse::Exists(o) => {
                let object: Object = o.try_into()?;
                let layout = match &object.data {
                    Data::Move(object) => {
                        self.load_object_transitive_deps(&object.type_).await?;
                        Some(
                            object
                                .get_layout(ObjectFormatOptions::default(), &self.module_cache)?,
                        )
                    }
                    Data::Package(_) => None,
                };
                GetObjectDataResponse::Exists(SuiParsedObject::try_from(object, layout)?)
            }
            GetRawObjectDataResponse::NotExists(id) => GetObjectDataResponse::NotExists(id),
            GetRawObjectDataResponse::Deleted(oref) => GetObjectDataResponse::Deleted(oref),
        })
    }

    // this function over-approximates
    // it loads all modules used in the type declaration
    // and then all of their dependencies.
    // To be exact, it would need to look at the field layout for each type used, but this will
    // be complicated with generics. The extra loading here is hopefully insignificant
    async fn load_object_transitive_deps(
        &self,
        struct_tag: &StructTag,
    ) -> Result<(), anyhow::Error> {
        fn used_packages(packages: &mut Vec<ObjectID>, type_: &TypeTag) {
            match type_ {
                TypeTag::Bool
                | TypeTag::U8
                | TypeTag::U64
                | TypeTag::U128
                | TypeTag::Address
                | TypeTag::Signer => (),
                TypeTag::Vector(inner) => used_packages(packages, inner),
                TypeTag::Struct(StructTag {
                    address,
                    type_params,
                    ..
                }) => {
                    packages.push((*address).into());
                    for t in type_params {
                        used_packages(packages, t)
                    }
                }
            }
        }
        let StructTag {
            address,
            type_params,
            ..
        } = struct_tag;
        let mut queue = vec![(*address).into()];
        for t in type_params {
            used_packages(&mut queue, t)
        }

        let mut seen: HashSet<ObjectID> = HashSet::new();
        while let Some(cur) = queue.pop() {
            if seen.contains(&cur) {
                continue;
            }
            let obj = self.get_object(cur).await?;
            let obj: Object = obj.into_object()?.try_into()?;
            let package = match &obj.data {
                Data::Move(_) => {
                    debug_assert!(false, "{cur} should be a package, not a move object");
                    continue;
                }
                Data::Package(package) => package,
            };
            let modules = package
                .serialized_module_map()
                .keys()
                .map(|name| package.deserialize_module(&Identifier::new(name.clone()).unwrap()))
                .collect::<Result<Vec<_>, _>>()?;
            for module in modules {
                let self_package_idx = module
                    .module_handle_at(module.self_module_handle_idx)
                    .address;
                let self_package = *module.address_identifier_at(self_package_idx);
                seen.insert(self_package.into());
                for handle in &module.module_handles {
                    let dep_package = *module.address_identifier_at(handle.address);
                    queue.push(dep_package.into());
                }
            }
        }
        Ok(())
    }

    pub async fn get_total_transaction_number(&self) -> anyhow::Result<u64> {
        Ok(match &*self.api {
            SuiClientApi::Rpc(c, _) => c.get_total_transaction_number().await?,
            SuiClientApi::Embedded(c) => c.get_total_transaction_number()?,
        })
    }

    pub async fn get_transactions_in_range(
        &self,
        start: GatewayTxSeqNumber,
        end: GatewayTxSeqNumber,
    ) -> anyhow::Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>> {
        Ok(match &*self.api {
            SuiClientApi::Rpc(c, _) => c.get_transactions_in_range(start, end).await?,
            SuiClientApi::Embedded(c) => c.get_transactions_in_range(start, end)?,
        })
    }

    pub async fn get_recent_transactions(
        &self,
        count: u64,
    ) -> anyhow::Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>> {
        Ok(match &*self.api {
            SuiClientApi::Rpc(c, _) => c.get_recent_transactions(count).await?,
            SuiClientApi::Embedded(c) => c.get_recent_transactions(count)?,
        })
    }

    pub async fn get_transaction(
        &self,
        digest: TransactionDigest,
    ) -> anyhow::Result<SuiTransactionResponse> {
        Ok(match &*self.api {
            SuiClientApi::Rpc(c, _) => c.get_transaction(digest).await?,
            SuiClientApi::Embedded(c) => c.get_transaction(digest).await?,
        })
    }
}

pub struct FullNodeApi(Arc<SuiClientApi>);

impl FullNodeApi {
    pub async fn get_transactions_by_input_object(
        &self,
        object: ObjectID,
    ) -> anyhow::Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>> {
        Ok(match &*self.0 {
            SuiClientApi::Rpc(c, _) => c.get_transactions_by_input_object(object).await?,
            SuiClientApi::Embedded(_) => {
                return Err(anyhow!("Method not supported by embedded gateway client."))
            }
        })
    }

    pub async fn get_transactions_by_mutated_object(
        &self,
        object: ObjectID,
    ) -> anyhow::Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>> {
        Ok(match &*self.0 {
            SuiClientApi::Rpc(c, _) => c.get_transactions_by_mutated_object(object),
            SuiClientApi::Embedded(_) => {
                return Err(anyhow!("Method not supported by embedded gateway client."))
            }
        }
        .await?)
    }

    pub async fn get_transactions_by_move_function(
        &self,
        package: ObjectID,
        module: Option<String>,
        function: Option<String>,
    ) -> anyhow::Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>> {
        Ok(match &*self.0 {
            SuiClientApi::Rpc(c, _) => {
                c.get_transactions_by_move_function(package, module, function)
            }
            SuiClientApi::Embedded(_) => {
                return Err(anyhow!("Method not supported by embedded gateway client."))
            }
        }
        .await?)
    }

    pub async fn get_transactions_from_addr(
        &self,
        addr: SuiAddress,
    ) -> anyhow::Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>> {
        Ok(match &*self.0 {
            SuiClientApi::Rpc(c, _) => c.get_transactions_from_addr(addr),
            SuiClientApi::Embedded(_) => {
                return Err(anyhow!("Method not supported by embedded gateway client."))
            }
        }
        .await?)
    }

    pub async fn get_transactions_to_addr(
        &self,
        addr: SuiAddress,
    ) -> anyhow::Result<Vec<(GatewayTxSeqNumber, TransactionDigest)>> {
        Ok(match &*self.0 {
            SuiClientApi::Rpc(c, _) => c.get_transactions_to_addr(addr),
            SuiClientApi::Embedded(_) => {
                return Err(anyhow!("Method not supported by embedded gateway client."))
            }
        }
        .await?)
    }
}
pub struct EventApi(Arc<SuiClientApi>);

impl EventApi {
    pub async fn subscribe_event(
        &self,
        filter: SuiEventFilter,
    ) -> anyhow::Result<impl Stream<Item = Result<SuiEventEnvelope, anyhow::Error>>> {
        match &*self.0 {
            SuiClientApi::Rpc(_, Some(c)) => {
                let subscription: Subscription<SuiEventEnvelope> =
                    c.subscribe_event(filter).await?;
                Ok(subscription.map(|item| Ok(item?)))
            }
            _ => Err(anyhow!("Subscription only supported by WebSocket client.")),
        }
    }
}

pub struct QuorumDriver {
    api: Arc<SuiClientApi>,
    state: Arc<RwLock<ClientState>>,
}

impl QuorumDriver {
    pub async fn execute_transaction(
        &self,
        tx: Transaction,
    ) -> anyhow::Result<SuiTransactionResponse> {
        let response = match &*self.api {
            SuiClientApi::Rpc(c, _) => {
                let tx_bytes = Base64::from_bytes(&tx.data.to_bytes());
                let flag = tx.tx_signature.scheme();
                let signature = Base64::from_bytes(tx.tx_signature.signature_bytes());
                let pub_key = Base64::from_bytes(tx.tx_signature.public_key_bytes());
                c.execute_transaction(tx_bytes, flag, signature, pub_key)
                    .await?
            }
            SuiClientApi::Embedded(c) => c.execute_transaction(tx).await?,
        };

        let mut all_changes = response
            .effects
            .mutated
            .iter()
            .map(|oref| oref.reference.clone())
            .collect::<Vec<_>>();
        all_changes.extend(response.effects.deleted.clone());
        let all_changes = all_changes
            .iter()
            .map(|oref| oref.to_object_ref())
            .collect::<Vec<_>>();
        self.state.write().await.update_refs(all_changes);

        Ok(response)
    }
}

impl SuiClient {
    pub fn transaction_builder(&self) -> &TransactionBuilder {
        &self.transaction_builder
    }
    pub fn read_api(&self) -> &ReadApi {
        &self.read_api
    }
    pub fn full_node_api(&self) -> &FullNodeApi {
        &self.full_node_api
    }
    pub fn event_api(&self) -> &EventApi {
        &self.event_api
    }
    pub fn quorum_driver(&self) -> &QuorumDriver {
        &self.quorum_driver
    }
    pub async fn sync_client_state(&self, address: SuiAddress) -> anyhow::Result<()> {
        match &*self.api {
            SuiClientApi::Rpc(c, _) => c.sync_account_state(address).await?,
            SuiClientApi::Embedded(c) => c.sync_account_state(address).await?,
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientType {
    Embedded(GatewayConfig),
    RPC(String, Option<String>),
}

impl Display for ClientType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut writer = String::new();

        match self {
            ClientType::Embedded(config) => {
                writeln!(writer, "Client Type : Embedded Gateway")?;
                writeln!(
                    writer,
                    "Gateway state DB folder path : {:?}",
                    config.db_folder_path
                )?;
                let authorities = config
                    .validator_set
                    .iter()
                    .map(|info| info.network_address());
                writeln!(
                    writer,
                    "Authorities : {:?}",
                    authorities.collect::<Vec<_>>()
                )?;
            }
            ClientType::RPC(url, ws_url) => {
                writeln!(writer, "Client Type : JSON-RPC")?;
                writeln!(writer, "HTTP RPC URL : {}", url)?;
                writeln!(writer, "WS RPC URL : {:?}", ws_url)?;
            }
        }
        write!(f, "{}", writer)
    }
}

impl ClientType {
    pub async fn init(&self) -> Result<SuiClient, anyhow::Error> {
        Ok(match self {
            ClientType::Embedded(config) => SuiClient::new_embedded_client(config)?,
            ClientType::RPC(url, ws_url) => {
                SuiClient::new_rpc_client(url, ws_url.as_deref()).await?
            }
        })
    }
}
