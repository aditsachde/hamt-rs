use cid::Cid as ExtCid;
use hamt_rs::{Cid, IpldHashMap};

use rayon::prelude::*;

use std::{path::PathBuf, time::Instant};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    block_db: PathBuf,
    tree_db: PathBuf,
    width: u8,
    bucket_size: u8,
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

    let tree = IpldHashMap::new(width.into(), bucket_size.into());
    let buckets = 2u8.pow(width.into());
    let iterations_per_prefix = 2u8.pow((8 - width).into());

    println!(
        "Starting insert! {} buckets {} iterations",
        buckets, iterations_per_prefix
    );
    let now = Instant::now();

    // Build up subtrees in parallel
    let iterator: Vec<u8> = (0..buckets).map(|x| x << (8 - width)).collect();
    println!("Prefixes: {:?}", iterator);

    let subtree_cids: Vec<(i64, Cid)> = iterator
        .par_iter()
        .map(|prefix| {
            let sled_tree = hash_keycid.clone();
            let mut tree = IpldHashMap::new(width.into(), bucket_size.into());
            let mut count = 0;

            for iteration in 0..iterations_per_prefix {
                let scan_prefix: &[u8] = &[prefix + iteration];
                println!("Prefix: {:?}, Iteration: {}", prefix, iteration);

                for key_cid in sled_tree.scan_prefix(scan_prefix) {
                    let key_cid = key_cid.unwrap().1;
                    let (key, cid): (&[u8], &[u8]) = bincode::deserialize(&key_cid).unwrap();
                    tree.set(
                        Vec::from(key).into_boxed_slice(),
                        Cid(ExtCid::try_from(cid).unwrap()),
                    )
                    .unwrap();
                    count += 1;
                }
            }

            println!("Collapsing prefix: {:?} Count: {}", prefix, count);
            let cid = tree.collapse_partial(&cid_tree.clone()).unwrap();

            drop(tree);

            (count, cid)
        })
        .collect();

    // Combine subtrees into a single tree
    let total: i64 = subtree_cids.iter().map(|(c, _)| c).sum();
    let subtree_cids = subtree_cids.into_iter().map(|(_, c)| c).collect();

    let cid = tree
        .serialize_root_of_subtrees(&cid_tree, subtree_cids)
        .unwrap();
    println!("Root CID: {} Count: {}", cid, total);

    let elapsed = now.elapsed();
    println!("Elapsed: {:.2?}", elapsed);
}
