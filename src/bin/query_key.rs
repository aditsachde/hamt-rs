use futures::TryStreamExt;
use hamt_rs::query::RootMapBlock;
use ipfs_api_backend_hyper::{IpfsApi, IpfsClient};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    root: String,
    key: String,
}

#[tokio::main]
async fn main() {
    let args = Cli::from_args();

    let client = IpfsClient::default();
    let root = &args.root;

    let block = client
        .block_get(root)
        .map_ok(|chunk| chunk.to_vec())
        .try_concat()
        .await
        .unwrap();

    let root: RootMapBlock = minicbor::decode(&block).unwrap();

    let response = root.get_key(args.key.as_bytes()).await;

    println!("{:?}", response);
}
