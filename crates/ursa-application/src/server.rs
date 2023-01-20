use crate::App;
use crate::ApplicationConfig;
use abci::async_api::Server;
use anyhow::Result;
use std::net::SocketAddr;

use clap::Parser;

// use tracing_error::ErrorLayer;

// use tracing_subscriber::prelude::*;

// /// Initializes a tracing Subscriber for logging
// #[allow(dead_code)]
// pub fn subscriber() {
//     tracing_subscriber::Registry::default()
//         .with(tracing_subscriber::EnvFilter::new("evm-app=trace"))
//         .with(ErrorLayer::default())
//         .with(tracing_subscriber::fmt::layer())
//         .init()
// }

pub async fn application_start(config: ApplicationConfig) -> Result<()> {
    let ApplicationConfig { domain, demo } = config;
    //  subscriber();

    let App {
        consensus,
        mempool,
        info,
        snapshot,
    } = App::new(demo);
    let server = Server::new(consensus, mempool, info, snapshot);

    dbg!(&domain);
    // let addr = args.host.strip_prefix("http://").unwrap_or(&args.host);
    let addr = domain.parse::<SocketAddr>().unwrap();

    // let addr = SocketAddr::new(addr, args.port);
    server.run(addr).await?;

    Ok(())
}
