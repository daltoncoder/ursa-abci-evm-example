use anyhow::{Result, bail};
use abci::{
    async_api::{
        Consensus as ConsensusTrait, Info as InfoTrait, Mempool as MempoolTrait,
        Snapshot as SnapshotTrait,
    },
    async_trait,
    types::*,
};
use revm::{self, Database, DatabaseCommit, db::{CacheDB, EmptyDB}, primitives::{Env, TxEnv, ExecutionResult}};
use std::sync::{Arc};
use bytes::Bytes;
use tokio::sync::Mutex;
use ethers::prelude::{NameOrAddress};
use ethers::types::{Address, TransactionRequest};
use revm::db::DatabaseRef;
use revm::primitives::{ CreateScheme, TransactTo, U256, AccountInfo, Bytecode, B160};
use hex::FromHex;


#[derive(Clone, Debug)]
pub struct State<Db> {
    pub block_height: i64,
    pub app_hash: Vec<u8>,
    pub db: Db,
    pub env: Env,
}

pub trait WithGenesisDb {
    fn insert_account_info(&mut self, address: B160, info: AccountInfo);
}

impl<Db: DatabaseRef> WithGenesisDb for CacheDB<Db> {
    #[inline(always)]
    fn insert_account_info(&mut self, address: B160, info: AccountInfo) {
        CacheDB::<Db>::insert_account_info(self, address, info);
    }
}

impl Default for State<CacheDB<EmptyDB>> {
    fn default() -> Self {
        Self {
            block_height: 0,
            app_hash: Vec::new(),
            db: CacheDB::new(EmptyDB()),
            env: Default::default(),
        }
    }
}

impl<Db: Database + DatabaseCommit> State<Db> {
    async fn execute(
        &mut self,
        tx: TransactionRequest,
        read_only: bool,
    ) -> Result<ExecutionResult> {
        let mut evm = revm::EVM::new();
        evm.env = self.env.clone();
        evm.env.tx = TxEnv {
            caller: tx.from.unwrap_or_default().to_fixed_bytes().into(),
            transact_to: match tx.to {
                Some(NameOrAddress::Address(inner)) => TransactTo::Call(inner.to_fixed_bytes().into()),
                Some(NameOrAddress::Name(_)) => bail!("not allowed"),
                None => TransactTo::Create(CreateScheme::Create),
            },
            data: tx.data.clone().unwrap_or_default().0,
            chain_id: Some(self.env.cfg.chain_id.try_into().unwrap()),
            nonce: Some(tx.nonce.unwrap_or_default().as_u64()),
            value: tx.value.unwrap_or_default().into(),
            gas_price: tx.gas_price.unwrap_or_default().into(),
            gas_priority_fee: Some(tx.gas_price.unwrap_or_default().into()),
            gas_limit: u64::MAX,
            access_list: vec![],
        };
        evm.database(&mut self.db);

        let results = match evm.transact() {
            Ok(data) => data,
            Err(err) => bail!("theres an err")
        };
        if !read_only {
            self.db.commit(results.state);
        };
        Ok(results.result)
    }
}

pub struct Consensus<Db> {
    pub committed_state: Arc<Mutex<State<Db>>>,
    pub current_state: Arc<Mutex<State<Db>>>,
}

impl<Db: Clone> Consensus<Db> {
    pub fn new(state: State<Db>) -> Self {
        let committed_state = Arc::new(Mutex::new(state.clone()));
        let current_state = Arc::new(Mutex::new(state));

        Consensus {
            committed_state,
            current_state,
        }
    }
}

#[async_trait]
impl<Db: Clone + Send + Sync + DatabaseCommit + Database + WithGenesisDb> ConsensusTrait for Consensus<Db> {
    #[tracing::instrument(skip(self))]
    async fn init_chain(&self, _init_chain_request: RequestInitChain) -> ResponseInitChain {
        tracing::trace!("initing the chain");
        let bytes = hex::decode("6080604052600436101561001257600080fd5b6000803560e01c80634f2be91f146100905780636d4ce63c1461006c5763c605f76c1461003f5750600080fd5b34610069575061004e366100bb565b610065610059610154565b604051918291826100cf565b0390f35b80fd5b5034610069576100659061007f366100bb565b546040519081529081906020820190565b503461006957506100a0366100bb565b6100656100ab610126565b6040519081529081906020820190565b60009060031901126100c957565b50600080fd5b919091602080825283519081818401526000945b828610610110575050806040939411610103575b601f01601f1916010190565b60008382840101526100f7565b85810182015184870160400152948101946100e3565b600054600019811461013c576001018060005590565b5050634e487b7160e01b600052601160045260246000fd5b604051906040820182811067ffffffffffffffff82111761019e57604052601e82527f48656c6c6f20576f726c642066726f6d20616e2075727361206e6f64652e00006020830152565b505050634e487b7160e01b600052604160045260246000fdfea3646970667358221220558eea9ea0fc897b48ba759f8f6cddb8d9472f66c8840242a0243442c4c2be966c6578706572696d656e74616cf564736f6c634300080c0041").unwrap();
        let bytecode = Bytecode::new_raw(Bytes::from(bytes).to_vec().into());
        let contract = AccountInfo {code: Some(bytecode.clone()),code_hash: bytecode.hash(), .. AccountInfo::default()};
        let mut state = self.current_state.lock().await;

        state.db.insert_account_info("0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".parse().unwrap(), contract);

        self.commit(RequestCommit{});

        ResponseInitChain::default()
    }

    #[tracing::instrument(skip(self))]
    async fn begin_block(&self, _begin_block_request: RequestBeginBlock) -> ResponseBeginBlock {
        ResponseBeginBlock::default()
    }

    #[tracing::instrument(skip(self))]
    async fn deliver_tx(&self, deliver_tx_request: RequestDeliverTx) -> ResponseDeliverTx {
        tracing::trace!("delivering tx");
        let mut state = self.current_state.lock().await;

        let mut tx: TransactionRequest = match serde_json::from_slice(&deliver_tx_request.tx) {
            Ok(tx) => tx,
            Err(_) => {
                tracing::error!("could not decode request");
                return ResponseDeliverTx {
                    data: "could not decode request".into(),
                    ..Default::default()
                };
            }
        };

        // resolve the `to`
        match tx.to {
            Some(NameOrAddress::Address(addr)) => tx.to = Some(addr.into()),
            _ => panic!("not an address"),
        };

        let result = state.execute(tx, false).await.unwrap();
        tracing::trace!("executed tx");

        ResponseDeliverTx {
            data: serde_json::to_vec(&result).unwrap(),
            ..Default::default()
        }
    }

    #[tracing::instrument(skip(self))]
    async fn end_block(&self, end_block_request: RequestEndBlock) -> ResponseEndBlock {
        tracing::trace!("ending block");
        let mut current_state = self.current_state.lock().await;
        current_state.block_height = end_block_request.height;
        current_state.app_hash = vec![];
        tracing::trace!("done");

        ResponseEndBlock::default()
    }

    #[tracing::instrument(skip(self))]
    async fn commit(&self, _commit_request: RequestCommit) -> ResponseCommit {
        tracing::trace!("taking lock");
        let current_state = self.current_state.lock().await.clone();
        let mut committed_state = self.committed_state.lock().await;
        *committed_state = current_state;
        tracing::trace!("committed");

        ResponseCommit {
            data: vec![], // (*committed_state).app_hash.clone(),
            retain_height: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mempool;

#[async_trait]
impl MempoolTrait for Mempool {
    async fn check_tx(&self, _check_tx_request: RequestCheckTx) -> ResponseCheckTx {
        ResponseCheckTx::default()
    }
}

#[derive(Debug, Clone)]
pub struct Info<Db> {
    pub state: Arc<Mutex<State<Db>>>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Query {
    EthCall(TransactionRequest),
    Balance(Address),
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum QueryResponse {
    Tx(ExecutionResult),
    Balance(U256),
}

impl QueryResponse {
    pub fn as_tx(&self) -> &ExecutionResult {
        match self {
            QueryResponse::Tx(inner) => inner,
            _ => panic!("not a tx"),
        }
    }

    pub fn as_balance(&self) -> U256 {
        match self {
            QueryResponse::Balance(inner) => *inner,
            _ => panic!("not a balance"),
        }
    }
}

#[async_trait]
impl<Db: Send + Sync + Database + DatabaseCommit> InfoTrait for Info<Db> {
    // replicate the eth_call interface
    async fn query(&self, query_request: RequestQuery) -> ResponseQuery {
        let mut state = self.state.lock().await;

        let query: Query = match serde_json::from_slice(&query_request.data) {
            Ok(tx) => tx,
            // no-op just logger
            Err(_) => {
                return ResponseQuery {
                    value: "could not decode request".into(),
                    ..Default::default()
                };
            }
        };

        let res = match query {
            Query::EthCall(mut tx) => {
                match tx.to {
                    Some(NameOrAddress::Address(addr)) => tx.to = Some(addr.into()),
                    _ => panic!("not an address"),
                };

                let result = state.execute(tx, true).await.unwrap();
                QueryResponse::Tx(result)
            }
            Query::Balance(address) => match state.db.basic(address.to_fixed_bytes().into()) {
                Ok(info) => QueryResponse::Balance(info.unwrap_or_default().balance.into()),
                _ => panic!("error retrieveing balance"),
            },
        };

        ResponseQuery {
            key: query_request.data,
            value: serde_json::to_vec(&res).unwrap(),
            ..Default::default()
        }
    }

    async fn info(&self, _info_request: RequestInfo) -> ResponseInfo {
        let state = self.state.lock().await;

        ResponseInfo {
            data: Default::default(),
            version: Default::default(),
            app_version: Default::default(),
            last_block_height: (*state).block_height,
            last_block_app_hash: (*state).app_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot;

impl SnapshotTrait for Snapshot {}

