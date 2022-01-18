use cid::{Cid, CidGeneric};
use hamt_rs::car::Car;
use indicatif::ProgressBar;
use minicbor::Encode;
use multihash::{Code, MultihashDigest};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter},
    path::PathBuf,
};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    tree_db: PathBuf,
    tree_car: PathBuf,
}

fn main() {
    let args = Cli::from_args();

    println!("Opening database");

    let cid_tree = sled::open(args.tree_db).unwrap();

    println!("Opening file");

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.tree_car)
        .unwrap();

    // 1 MB buffer size
    let file = BufWriter::with_capacity(128 * 1024, file);

    let mut generic_car = Car::new(Box::new(file));
    generic_car.encode_header().unwrap();

    println!("Starting to write car");

    let mut count = 0;
    for entry in cid_tree.iter(){
        let (cid, block) = entry.unwrap();
        generic_car.write_block(&cid, &block).unwrap();
        count += 1;

        if (count % 100000) == 0 {
            println!("Progress: {}", count);
        } 
    }
    println!("Total: {} ", count);
}
