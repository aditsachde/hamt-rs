use async_recursion::async_recursion;
use bitvec::prelude::*;
use futures::{stream::Map, StreamExt, TryStreamExt};
use hamt_rs::{to_int, Cid};
use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, request::{DagGet, DagCodec}};
use minicbor::Decode;
use multihash::{Code, MultihashDigest};
use std::ops::Deref;
use hamt_rs::query::RootMapBlock;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    root: String,
    key: String
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

    let response = root.get_key(b"/authors/OL100025A").await;

    println!("{:?}", response);
}