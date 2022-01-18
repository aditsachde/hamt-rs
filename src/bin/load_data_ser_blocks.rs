use cid::Cid;
use hamt_rs::{car::Car, Value};
use indicatif::ParallelProgressIterator;
use multihash::{Code, MultihashDigest};
use rayon::prelude::*;

use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    block_db: PathBuf,
    file: PathBuf,
    records: u64,
    block_car: Option<PathBuf>,
}

type ChannelVal = (Vec<u8>, Vec<u8>);

fn main() {
    let args = Cli::from_args();

    let write_to_car = args.block_car.is_some();

    if write_to_car {
        // Reserve one thread for the filewriter
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get() - 1)
            .build_global()
            .unwrap();
    }

    let (tx, rx): (Sender<ChannelVal>, Receiver<ChannelVal>) = mpsc::channel();

    let db = sled::Config::new()
        .path(args.block_db)
        .flush_every_ms(Some(5000))
        .mode(sled::Mode::HighThroughput)
        .open()
        .unwrap();
    let hash_keycid = db.open_tree("hash_keycid").unwrap();

    // Setup thread for writing out the .car file
    let writer = thread::spawn(move || {
        if let Some(block_car) = args.block_car {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(block_car)
                .unwrap();

            let file = BufWriter::with_capacity(128 * 1024, file);
            let mut generic_car = Car::new(Box::new(file));
            generic_car.encode_header().unwrap();

            for _ in 0..args.records {
                let (cid, block) = rx.recv().unwrap();
                generic_car.write_block(&cid, &block).unwrap();
            }
        }
    });

    let file = File::open(args.file).unwrap();
    let file = BufReader::with_capacity(128 * 1024, file);

    // Process records in parallel
    file.lines()
        .par_bridge()
        .progress_count(args.records)
        .map(|line| {
            let line = line.unwrap();
            /**************** MODIFY BELOW *****************/
            // Split line at tabs
            let mut record = line.split('\t');
            // Key is the 2nd column
            let key = record.nth(1).unwrap();
            // Json is the 5th column (so move 3 columns)
            let json = record.nth(2).unwrap();
            /**************** MODIFY ABOVE *****************/

            let record = Value(serde_json::from_str(json).unwrap());
            let block = minicbor::to_vec(record).unwrap();

            let keybytes = key.as_bytes();
            let keyhash = Code::Sha2_256.digest(keybytes);
            let keyhashdigest = keyhash.digest();

            // 0x71 - dag_cbor - https://github.com/multiformats/multicodec/blob/master/table.csv#L44
            let cid = Cid::new_v1(0x71, Code::Sha2_256.digest(&block));
            let cidbytes = cid.to_bytes();

            let keycid = bincode::serialize(&(keybytes, &cidbytes)).unwrap();
            hash_keycid.insert(keyhashdigest, keycid).unwrap();

            (cidbytes, block)
        })
        .try_for_each_with(
            tx,
            |tx, value| {
                if write_to_car {
                    tx.send(value)
                } else {
                    Ok(())
                }
            },
        )
        .expect("expected no send errors");

    writer.join().unwrap();
}
