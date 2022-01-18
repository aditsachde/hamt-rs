use hamt_rs::car::Car;
use std::{fs::OpenOptions, io::BufWriter, path::PathBuf};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    block_db: PathBuf,
    block_car: PathBuf,
}

fn main() {
    let args = Cli::from_args();

    let db = sled::open(args.block_db).unwrap();
    let cid_db = db.open_tree("cid_db").unwrap();

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.block_car)
        .unwrap();

    let file = BufWriter::with_capacity(128 * 1024, file);

    let mut generic_car = Car::new(Box::new(file));
    generic_car.encode_header().unwrap();

    let mut count = 0;
    for entry in cid_db.iter() {
        let (cid, block) = entry.unwrap();
        generic_car.write_block(&cid, &block).unwrap();
        count += 1;

        if (count % 1000000) == 0 {
            println!("Progress: {}", count);
        }
    }
    println!("{} ", count);
}
