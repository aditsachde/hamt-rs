use cid::Cid as ExtCid;
use hamt_rs::{Cid, IpldHashMap, Value};
use indicatif::ProgressIterator;
use minicbor::Encode;
use multihash::{Code, MultihashDigest};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufRead, BufReader},
    ops::Deref,
    path::PathBuf,
    time::Instant
};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    block_db: PathBuf,
    tree_db: PathBuf,
    width: u8,
    bucket_size: u8
}

fn main() {
    let args = Cli::from_args();

    let width = args.width;
    let bucket_size = args.bucket_size;

    let db = sled::open(args.block_db).unwrap();
    let hash_keycid = db.open_tree("hash_keycid").unwrap();

    let cid_tree = sled::Config::new()
        .path(args.tree_db)
        .flush_every_ms(Some(1000))
        .mode(sled::Mode::HighThroughput)
        .open()
        .unwrap();

    cid_tree.clear().unwrap();

    let mut tree = IpldHashMap::new(width.into(), bucket_size.into());

    let now = Instant::now();
    let mut count: i64 = 0;

    for hash_keycid in hash_keycid.iter().progress_count(2138824) {
        let hash_keycid = hash_keycid.unwrap().1;
        let (key, cid): (&[u8], &[u8]) = bincode::deserialize(&hash_keycid).unwrap();
        tree.set(
            Vec::from(key).into_boxed_slice(),
            Cid(ExtCid::try_from(cid).unwrap()),
        )
        .unwrap();
        count += 1;
    }

    let elapsed = now.elapsed();
    println!("Elapsed: {:.2?}", elapsed);

    let now = Instant::now();

    let cid = tree.collapse(&cid_tree);

    let elapsed = now.elapsed();
    println!("Elapsed: {:.2?}", elapsed);

    println!("Root CID: {} Count: {}", cid, count);
}
