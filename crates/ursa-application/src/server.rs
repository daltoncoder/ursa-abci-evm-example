use crate::App;
use crate::ApplicationConfig;
use abci::async_api::Server;
use anyhow::Result;
use std::net::SocketAddr;

pub async fn application_start(config: ApplicationConfig) -> Result<()> {
    let ApplicationConfig { domain } = config;
    //  subscriber();

    let App {
        consensus,
        mempool,
        info,
        snapshot,
    } = App::new();
    let server = Server::new(consensus, mempool, info, snapshot);

    dbg!(&domain);
    // let addr = args.host.strip_prefix("http://").unwrap_or(&args.host);
    let addr = domain.parse::<SocketAddr>().unwrap();

    // let addr = SocketAddr::new(addr, args.port);
    server.run(addr).await?;

    Ok(())
}
