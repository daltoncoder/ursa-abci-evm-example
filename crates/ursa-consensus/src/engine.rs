use crate::{AbciApi, AbciQueryQuery, ConsensusConfig};
use anyhow::{bail, Context, Result};
use db::{rocks::RocksDb, rocks_config::RocksDbConfig};
use libp2p::identity::ed25519::PublicKey;
use libp2p::identity::Keypair as LibKeypair;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc::{channel, Receiver};
use tokio::sync::oneshot::Sender as OneShotSender;
use tracing::warn;

// Tendermint Types
use tendermint_abci::{Client as AbciClient, ClientBuilder};
use tendermint_proto::abci::{
    RequestBeginBlock, RequestDeliverTx, RequestEndBlock, RequestInfo, RequestInitChain,
    RequestQuery, ResponseQuery,
};
use tendermint_proto::types::Header;

// Narwhal types
use narwhal_config::{Committee, Import as _, KeyPair, Parameters};
use narwhal_consensus::Consensus;
use narwhal_crypto::{Digest, PublicKey as NarPublicKey};
use narwhal_primary::{Certificate, Primary};
use narwhal_store::Store;
use narwhal_worker::Worker;

/// The default channel capacity.
pub const CHANNEL_CAPACITY: usize = 1_000;

/// The engine drives the ABCI Application by concurrently polling for:
/// 1. Calling the BeginBlock -> DeliverTx -> EndBlock -> Commit event loop on the ABCI App on each Bullshark
///    certificate received. It will also call Info and InitChain to initialize the ABCI App if
///    necessary.
/// 2. Processing Query & Broadcast Tx messages received from the Primary's ABCI Server API and forwarding them to the
///    ABCI App via a Tendermint protobuf client.
pub struct Engine {
    /// The address of the ABCI app
    pub app_address: SocketAddr,
    /// The path to the Primary's store, so that the Engine can query each of the Primary's workers
    /// for the data corresponding to a Certificate
    pub store_path: String,
    /// Messages received from the ABCI Server to be forwarded to the engine.
    pub rx_abci_queries: Receiver<(OneShotSender<ResponseQuery>, AbciQueryQuery)>,
    /// The last block height, initialized to the application's latest block by default
    pub last_block_height: i64,
    pub client: AbciClient,
    pub req_client: AbciClient,
}

impl Engine {
    pub fn new(
        app_address: SocketAddr,
        store_path: &str,
        rx_abci_queries: Receiver<(OneShotSender<ResponseQuery>, AbciQueryQuery)>,
    ) -> Self {
        let mut client = ClientBuilder::default().connect(&app_address).unwrap();

        let last_block_height = client
            .info(RequestInfo::default())
            .map(|res| res.last_block_height)
            .unwrap_or_default();

        // Instantiate a new client to not be locked in an Info connection
        let client = ClientBuilder::default().connect(&app_address).unwrap();
        let req_client = ClientBuilder::default().connect(&app_address).unwrap();
        Self {
            app_address,
            store_path: store_path.to_string(),
            rx_abci_queries,
            last_block_height,
            client,
            req_client,
        }
    }

    /// Receives an ordered list of certificates and apply any application-specific logic.
    pub async fn run(&mut self, mut rx_output: Receiver<Certificate>) -> Result<()> {
        self.init_chain()?;

        loop {
            tokio::select! {
                Some(certificate) = rx_output.recv() => {
                    self.handle_cert(certificate)?;
                },
                Some((tx, req)) = self.rx_abci_queries.recv() => {
                    self.handle_abci_query(tx, req)?;
                }
                else => break,
            }
        }

        Ok(())
    }

    /// On each new certificate, increment the block height to proposed and run through the
    /// BeginBlock -> DeliverTx for each tx in the certificate -> EndBlock -> Commit event loop.
    fn handle_cert(&mut self, certificate: Certificate) -> Result<()> {
        // increment block
        let proposed_block_height = self.last_block_height + 1;

        // save it for next time
        self.last_block_height = proposed_block_height;

        // drive the app through the event loop
        self.begin_block(proposed_block_height)?;
        self.reconstruct_and_deliver_txs(certificate)?;
        self.end_block(proposed_block_height)?;
        self.commit()?;
        Ok(())
    }

    /// Handles ABCI queries coming to the primary and forwards them to the ABCI App. Each
    /// handle call comes with a Sender channel which is used to send the response back to the
    /// Primary and then to the client.
    ///
    /// Client => Primary => handle_cert => ABCI App => Primary => Client
    fn handle_abci_query(
        &mut self,
        tx: OneShotSender<ResponseQuery>,
        req: AbciQueryQuery,
    ) -> Result<()> {
        let req_height = req.height.unwrap_or(0);
        let req_prove = req.prove.unwrap_or(false);

        let resp = self.req_client.query(RequestQuery {
            data: req.data.into(),
            path: req.path,
            height: req_height as i64,
            prove: req_prove,
        })?;

        if let Err(err) = tx.send(resp) {
            bail!("{:?}", err);
        }
        Ok(())
    }

    /// Opens a RocksDB handle to a Worker's database and tries to read the batch
    /// stored at the provided certificate's digest.
    fn reconstruct_batch(&self, digest: Digest, worker_id: u32) -> Result<Vec<u8>> {
        // Open the database to each worker
        // TODO: Figure out if this is expensive
        // Should this be read only?
        let worker_store = RocksDb::open(
            PathBuf::from(self.worker_db(worker_id)),
            &RocksDbConfig::default(),
        )?;

        // Query the db
        let key = digest.to_vec();
        match worker_store.db.get(&key) {
            Ok(Some(res)) => Ok(res),
            Ok(None) => bail!("digest {} not found", digest),
            Err(err) => bail!(err),
        }
    }

    /// Calls DeliverTx on the ABCI app
    /// Deserializes a raw abtch as `WorkerMesssage::Batch` and proceeds to deliver
    /// each transaction over the DeliverTx API.
    fn deliver_batch(&mut self, batch: Vec<u8>) -> Result<()> {
        // Deserialize and parse the message.
        match bincode::deserialize(&batch) {
            Ok(WorkerMessage::Batch(batch)) => {
                batch.into_iter().try_for_each(|tx| {
                    self.deliver_tx(tx)?;
                    Ok::<_, anyhow::Error>(())
                })?;
            }
            _ => bail!("unrecognized message format"),
        };
        Ok(())
    }

    /// Reconstructs the batch corresponding to the provided Primary's certificate from the Workers' stores
    /// and proceeds to deliver each tx to the App over ABCI's DeliverTx endpoint.
    fn reconstruct_and_deliver_txs(&mut self, certificate: Certificate) -> Result<()> {
        // Try reconstructing the batches from the cert digests
        //
        // NB:
        // This is maybe a false positive by Clippy, without the `collect` the Iterator fails
        // iterator fails to compile because we're mutably borrowing in the `try_for_each`
        // when we've already immutably borrowed in the `.map`.
        #[allow(clippy::needless_collect)]
        let batches = certificate
            .header
            .payload
            .into_iter()
            .map(|(digest, worker_id)| self.reconstruct_batch(digest, worker_id))
            .collect::<Vec<_>>();

        // Deliver
        batches.into_iter().try_for_each(|batch| {
            // this will throw an error if the deserialization failed anywhere
            let batch = batch?;
            self.deliver_batch(batch)?;
            Ok::<_, anyhow::Error>(())
        })?;

        Ok(())
    }

    /// Helper function for getting the database handle to a worker associated
    /// with a primary (e.g. Primary db-0 -> Worker-0 db-0-0, Wroekr-1 db-0-1 etc.)
    fn worker_db(&self, id: u32) -> String {
        format!("{}-{}", self.store_path, "worker")
    }
}

// Tendermint Lifecycle Helpers
impl Engine {
    /// Calls the `InitChain` hook on the app, ignores "already initialized" errors.
    ///
    /// TODO: this is where we should load a genesis file
    pub fn init_chain(&mut self) -> Result<()> {
        let mut client = ClientBuilder::default().connect(&self.app_address)?;
        match client.init_chain(RequestInitChain::default()) {
            Ok(_) => {}
            Err(err) => {
                // ignore errors about the chain being uninitialized
                if err.to_string().contains("already initialized") {
                    warn!("{}", err);
                    return Ok(());
                }
                bail!(err)
            }
        };
        Ok(())
    }

    /// Calls the `BeginBlock` hook on the ABCI app. For now, it just makes a request with
    /// the new block height.
    // If we wanted to, we could add additional arguments to be forwarded from the Consensus
    // to the App logic on the beginning of each block.
    fn begin_block(&mut self, height: i64) -> Result<()> {
        let req = RequestBeginBlock {
            header: Some(Header {
                height,
                ..Default::default()
            }),
            ..Default::default()
        };

        self.client.begin_block(req)?;
        Ok(())
    }

    /// Calls the `DeliverTx` hook on the ABCI app.
    fn deliver_tx(&mut self, tx: Transaction) -> Result<()> {
        self.client.deliver_tx(RequestDeliverTx { tx })?;
        Ok(())
    }

    /// Calls the `EndBlock` hook on the ABCI app. For now, it just makes a request with
    /// the proposed block height.
    // If we wanted to, we could add additional arguments to be forwarded from the Consensus
    // to the App logic on the end of each block.
    fn end_block(&mut self, height: i64) -> Result<()> {
        let req = RequestEndBlock { height };
        self.client.end_block(req)?;
        Ok(())
    }

    /// Calls the `Commit` hook on the ABCI app.
    fn commit(&mut self) -> Result<()> {
        self.client.commit()?;
        Ok(())
    }
}

// Helpers for deserializing batches, because `narwhal::worker` is not part
// of the public API. TODO -> make a PR to expose it.
pub type Transaction = Vec<u8>;
pub type Batch = Vec<Transaction>;
#[derive(serde::Deserialize)]
pub enum WorkerMessage {
    Batch(Batch),
}

//TODO: this start function probably would better live in the ursa crate, along with the process function
pub async fn consensus_start(
    config: ConsensusConfig,
    app_api: String,
    key: LibKeypair,
) -> Result<()> {
    let parameters = config.narwhal_params;
    let keypair: KeyPair;
    if let LibKeypair::Ed25519(ed_key) = key {
        keypair = KeyPair::load(ed_key.public().encode(), ed_key.encode());
    } else {
        bail!("Keypair not supported");
    }

    //Read the committe from a file
    let committee = Committee::import(&config.committee_path)
        .with_context(|| "Failed to read the committee data for committee file")?;

    //Make the primary data store+ worker datastore
    let primary_store =
        Store::new(&config.database_path).with_context(|| "Failed to create primary store")?;
    let worker_store = Store::new(&format!("{}-{}", config.database_path, "worker"))
        .with_context(|| "Failed to create worker store")?;
    // Channels the sequence of certificates.
    let (tx_output, mut rx_output) = channel(CHANNEL_CAPACITY);

    //Spawn the primary and and consensus core

    let (tx_new_certificates, rx_new_certificates) = channel(CHANNEL_CAPACITY);
    let (tx_feedback, rx_feedback) = channel(CHANNEL_CAPACITY);

    let keypair_name = keypair.name;

    Primary::spawn(
        keypair,
        committee.clone(),
        parameters.clone(),
        primary_store.clone(),
        /* tx_consensus */ tx_new_certificates,
        /* rx_consensus */ rx_feedback,
    );

    Consensus::spawn(
        committee.clone(),
        parameters.gc_depth,
        /* rx_primary */ rx_new_certificates,
        /* tx_primary */ tx_feedback,
        tx_output,
    );

    //spawn worker

    //for now we just spawn one worker but there could be more
    let id: u32 = 0;
    let new_keypair = keypair_name.clone();
    let new_committee = committee.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        Worker::spawn(
            new_keypair,
            0,
            new_committee,
            parameters,
            worker_store.clone(),
        );

        ()
    });

    process(
        rx_output,
        &config.database_path,
        keypair_name,
        committee,
        config.rpc_domain,
        app_api,
    )
    .await?;
    handle.await.unwrap();
    unreachable!();
}

async fn process(
    rx_output: Receiver<Certificate>,
    store_path: &str,
    keypair_name: NarPublicKey,
    committee: Committee,
    abci_api: String,
    app_api: String,
) -> Result<()> {
    // address of mempool
    let mempool_address = committee
        .worker(&keypair_name.clone(), &0)
        .expect("Our public key or worker id is not in the committee")
        .transactions;

    //3004
    warn!("DDD app_api = {}", app_api);
    //3007
    warn!("DDD mempool = {}", mempool_address);

    // ABCI queries will be sent using this from the RPC to the ABCI client
    let (tx_abci_queries, rx_abci_queries) = channel(CHANNEL_CAPACITY);

    tokio::spawn(async move {
        let api = AbciApi::new(mempool_address, tx_abci_queries);
        // let tx_abci_queries = tx_abci_queries.clone();
        // Spawn the ABCI RPC endpoint


        let mut address = abci_api.parse::<SocketAddr>().unwrap();
        address.set_ip("0.0.0.0".parse().unwrap());
        //3005
        warn!("DDD address = {}", address);
        warp::serve(api.routes()).run(address).await
    });

    // Analyze the consensus' output.
    // Spawn the network receiver listening to messages from the other primaries.
    let mut app_address = app_api.parse::<SocketAddr>().unwrap();
    app_address.set_ip("0.0.0.0".parse().unwrap());
    //3004
    warn!("DDD app_address = {}", app_address);
    let mut engine = Engine::new(app_address, store_path, rx_abci_queries);
    engine.run(rx_output).await?;

    Ok(())
}
