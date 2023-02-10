use anyhow::Result;
use ethers::abi::{AbiEncode, parse_abi};
use ethers::contract::BaseContract;
use ethers::prelude::{Address, NameOrAddress, TransactionRequest};
use bytes::Bytes;
use revm::primitives::{ExecutionResult, Output};
use ursa_application::types::{Query, QueryResponse};
use std::env;

#[tokio::main]
async fn main() -> Result<()>{
//     let contract_addr = "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".parse::<Address>().unwrap();
//     // generate abi for the calldata from the human readable interface
//     let abi = BaseContract::from(
//         parse_abi(&[
//             "function helloWorld() public pure returns (string)",
//         ])?
//     );
//     let encoded = abi.encode("helloWorld", ())?;
//
//     let transaction_request = TransactionRequest::new().to(contract_addr).data(Bytes::from(hex::decode(hex::encode(&encoded))?));
//
//     let query = Query::EthCall(transaction_request);
//
//     let query = serde_json::to_string(&query)?;
//     let client = reqwest::Client::new();
//     let res = client
//         .get(format!("{}/abci_query", "http://127.0.0.1:3005"))
//         .query(&[("data", query), ("path", "".to_string())])
//         .send()
//         .await?;
//
//     println!("res:{:?}", res);
//
//     let val = res.bytes().await?;
//
//     println!("{:?}", String::from_utf8_lossy(val.to_vec().as_slice()));
// let val: QueryResponse = serde_json::from_slice(&val)?;
//
//     let val = match val {
//         QueryResponse::Tx(res) => {
//             match res{
//                 ExecutionResult::Success {output,..} => {
//                     match output {
//                         Output::Call(bytes) => bytes,
//                         _ => panic!("Output wrong")
//                     }
//                 }
//                 _ => panic!("Txn was not succesful")
//             }
//         }
//         _ => panic!("Error")
//     };
//     println!("val3: {:?}", val);
//
//     let readable_output: String = match abi.decode_output("helloWorld", val){
//         Ok(output)=> output,
//         Err(e) => panic!("{:?}", e)
//     };
//     println!(
//         "Call results: {}",
//         readable_output
//     );
//
//     Ok(())
    let args: Vec<String> = env::args().collect();

    let contract_addr = "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".parse::<Address>().unwrap();

     // generate abi for the calldata from the human readable interface
     let abi = BaseContract::from(
         parse_abi(&[
             "function helloWorld() public pure returns (string)",
             "function get() public view returns (uint256)",
             "function add() external returns (uint256)"
         ])?
     );

    match args[1].as_str() {
        "get" => {
            let encoded = abi.encode("get", ())?;
            let transaction_request = TransactionRequest::new().to(contract_addr).data(Bytes::from(hex::decode(hex::encode(&encoded))?));
            let query = Query::EthCall(transaction_request);

    let query = serde_json::to_string(&query)?;
    let client = reqwest::Client::new();
    let res = client
       .get(format!("{}/abci_query", "http://127.0.0.1:3005"))
        .query(&[("data", query), ("path", "".to_string())])
         .send()
        .await?;

            let val = res.bytes().await?;

 let val: QueryResponse = serde_json::from_slice(&val)?;

     let val = match val {
        QueryResponse::Tx(res) => {
            match res{
                ExecutionResult::Success {output,..} => {
                    match output {
                        Output::Call(bytes) => bytes,
                        _ => panic!("Output wrong")
                    }
                }
                _ => panic!("Txn was not succesful")
             }
        }
         _ => panic!("Error")
     };

   let readable_output: u64 = match abi.decode_output("get", val){
        Ok(output)=> output,
         Err(e) => panic!("{:?}", e)
     };
     println!(
         "Counter is currently at: {}",
         readable_output
     );

        }
        "inc" => {
            let encoded = abi.encode("get", ())?;
            let transaction_request = TransactionRequest::new().to(contract_addr).data(Bytes::from(hex::decode(hex::encode(&encoded))?)).gas(21000000).from("0xDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".parse::<Address>().unwrap());

            let tx = serde_json::to_string(&transaction_request)?;

            let client = reqwest::Client::new();
            client
                .get(format!("{}/broadcast_tx", "http://127.0.0.1:3005"))
                .query(&[("tx", tx)])
                .send()
                .await?;

            println!("transaction sent to consensus");

        }
        _ => {
        panic!("Please add get or inc arg");
        }

    }
    Ok(())
}
