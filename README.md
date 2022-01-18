# Usage
Building a HAMT is done in three steps. First, clone the repo and run `cargo build --release`

**Load the Data**
Start by modifying <Line> in `src/bin/load_data_ser_blocks.rs` to match the format of your dataset.

Run the following command. The last arg is optional, which is useful when building multiple HAMTs with different keys from the same dataset. 
```
target/release/load_data_ser_blocks <block_db> <data file> <number of records> <block car>
```

**Build the Tree**
Run the following command. Note down the Root CID outputted at the end of this step, as the CID outputted when importing the generated .car files is not the same as this. 
```
target/release/build_tree_par <block_db> <tree_db> <width> <bucket size>
```

For small HAMTs (less than a thousand or so entries), this may result in an error. Instead, run the single threaded version.
```
target/release/build_tree <block_db> <tree_db> <width> <bucket size>
```

**Serialize the Tree**
```
target/release/serialize_tree_car <tree_db> <tree car>
```

**Run a query**
This can be done using any standard HAMT library in any language. However, there is a small demo provided. It requires a local IPFS daemon.

Import the block and tree cars generated from the previous steps into the local node by doing `ipfs dag import <block car>` and `ipfs dag import <tree car>`. Make sure that the local IPFS daemon is using the badger block store (`ipfs init --profile=badgerds`) as the import will be extremely slow otherwise.

```
target/release/query_key <root_cid> <key>
```

If this record exists in the HAMT, a CID will be returned. `ipfs dag get <cid>` can be used to retrieve the corresponding JSON record.

*Datasets*
There are two pregenerated datasets avaliable. They can be directly queried without importing the car files from the provided locations as the blocks are present on the IPFS network. However, this will likely be unusably slow depending on the number of copies of the tree present in the network.

About two million records, avaliable on the releases page.
- Root CID: `bafyreiatlxnr6ilfyxuphywglyxsnxzobfpxk5fbjk4lef7bawirbsp2x4`

All records (over 68 million), avaliable on IPFS as `QmXiGQJn1JHNp339WUvPAtqfcRh9u1vdUp8L5s2HtDVqQE`
- Root CID: `bafyreiblsloewhro5civi25yptqb2mosxkpc6py6be6o77rnv4krsblymq`

# Details
The goal of this project was to demonstrate and provide tools for querying a dataset on IPFS by some key. Generally with IPFS, a client would download a dataset, such as a sqlite file, and then perform a lookup. This works fine for small datasets, but what about very large ones, such as the complete Open Library data dump.

IPFS provides a solution to this in the form of Interplantery Linked Data, and one its specs, a hash map array trie (HAMT), adapted for IPFS. The concept is quite interesting, but actually using it can be difficult, especially for such a large dataset.

IPLD's HAMT can be thought of as a merkle tree where operations are performed on the hash of the key to provide even distribution. This project ended up being implemented three different times to provide acceptable performance for the demonstration dataset, the Open Library dump. It is 50GB uncompressed, with over 68 million records.

## The Straightfoward Way
Follow the spec exactly. This works, but its not very useful for such a large HAMT. For every insert, the node is serialized, its parent is updated with the new hash, the parent is serialized, and so on and so forth up to the root. The very next insert does the same thing and changes the hash. Unless we need every intermediate stage, we do a lot of work that is immediatly thrown away.

## The Slightly Better Way
Model the tree as such. Implement it as a traditional HAMT in memory, using pointers instead of hashes the way IPLD requires. Once all entries are inserted, convert it in a single step.

```rust
pub struct Node<V: Serialize> {
    pub map: BitVec<Lsb0, u8>,
    pub data: Vec<Element<V>>,
}

pub enum Element<V: Serialize> {
    HashMapNode(HashMapNode<V>),
    Bucket(Vec<BucketEntry<V>>),
}

struct BucketEntry<V: Serialize> {
    key: Vec<u8>,
    value: V,
}
```

This is better, but it is still slow. The obvious issues are that its single threaded and memory usage will be very high, since everything gets loaded into memory. The biggest issue is pointer chasing. To traverse a node, the map has to be fetched, then the data vector, and then finally the next node. Three pointers have to be dereferenced per node, and for every insertion, there are multiple nodes that are traversed. 

Why does this end up being such an issue though? How come langages like Java can be fast while still storing everything on the heap behind pointers? It turns out fetching something from main memory is very slow compared on CPU caches, and HAMTs destroy their hit ratios. Even if keys are sequentially inserted, they are inserted in the tree based on their hash, which is essentially random. This means that for every insert, a random set of nodes are needed and caches are quite small, so nodes tend to be evicted before they're used again.

## The Extra Memory Intensive Way
Model the tree as such. Rust recently gained support for const generics, allowing structs to hold arrays with sizes known at compile time, allowing them to be inlined instead of requiring a pointer.

```rust
pub struct Node<V, const N: usize> {
    pub map: [bool; N],
    pub data: [Element<V>; N],
}

pub enum Element<V, const N: usize> {
    HashMapNode(HashMapNode<V, N>),
    Bucket(Vec<BucketEntry<V>>),
}

struct BucketEntry<V, const N: usize> {
    key: Vec<u8>,
    value: V,
}
```

This solves the entire pointer chasing issue. However, each node ends up being massive (10s of KB), despite most of the nodes being almost empty. In the previous scenario, an on disk store for nodes would have been nice, but its possible to still have enough memory on a standard computer to use it for a large dataset. For this, an on disk store for nodes is required to be useful (or a VPS with hundreds of gigs of ram). Additionally, as mentioned in the previous section, due to the randomness of the key hashes, a couple blocks would inevitably end up having to be fetched from disk.

## So now what? The Implemented Way
All of these ways have some sort of issue that causes slowdowns or a lot of complexity. At this point, it was time to take a step back and reevaluate the project. The original goal was a complete IPLD HAMT library that could support building large trees, perform partial updates to an existing HAMT, and a variety of other supporting functions efficiently. More details are in the lessons learned section, but I decided to focus on doing a small portion well.

Like many others, the Open Library dataset is a bunch of JSON records. Instead of storing the record directly in the HAMT, it is better to first serialize the record into its own document, and then store the CID in the HAMT. This way, its possible to have multiple HAMTs with different keys for lookup backed by the same, deduplicated dataset in IPFS.

The steps for building a HAMT are as follows:
1. Serialize the record and get its CID.
2. Hash the key.
3. Traverse the tree by the hash and insert the key and CID.
4. Repeat for every record.
5. Hash every block and covert the tree to a spec complient IPLD HAMT.

Lets reorder the steps:
1. Serialize every record and store CIDs.
2. Hash every key and store them.
3. Insert every key CID pair, iterating through them ordered by the hash.
4. Hash every block and covert the tree to a spec complient IPLD HAMT.

This only does a small portion of the previous approaches, but it fixes all the issues for that portion. Steps 1 and 2 can be trivially parallerlized. Due to step 3, we can go back to modeling nodes with vectors and pointers. Since inserts are done by hash order, the correct nodes end up in cache and pointers are fast again. 

A HAMT node also has multiple children. Iterating by hash order also makes it obvious upfront which child of the root a node will go into. Therefore, each child of the root can be treated as its own tree, each of which can be done on its own core, allowing for parallelism. 

For a dataset of 8 million keys, the preprocessing step took about 1 minute 30 seconds. Building the tree itself took under 25 seconds.

Unfortunatly, the preprocessign step does not scale linearly. On my machine, due to some combination of sled (the underlying KV store), the way B-trees work, and a relativily slow ssd, for a dataset of 68 million keys, the preprocessing step took around 35 minutes. Building the tree itself took a bit under 5 minutes. 

Note that the preprocessing step is required. Sled stores the KV pairs sorted lexiographically. Since hashes are random, doing this requires a lot of additional work. However, the lexiographic ordering is what makes everything else possible to do in a reasonable amount of time. Additionally, the preprocessing step includes serializing the dataset to a car file. This part can be omitted on subsequent runs, speeding up this step.

Overall, I'm happy with this. It is still much, much faster than the other implementations. There are still various places where it can be algorithemically improved, but I choose not to implement these as the bottleneck was IO.

# Lessons Learned
1. Rust is not the right language for a general HAMT library. The IPFS ecosystem is currently focused on Go and JS. There has been efforts to bring it over to Rust, but its still early days. Its also still early days for a lot of the advanced IPLD features, such as path selectors. These are important, because it has the potential to speed up querying a HAMT over a network by many multiples. These features are being prototyped in Go, making it the right language to use for building something on IPLD due to the ecosystem.
2. Rust is not the right language for a general HAMT library. The Rust type system is very strict and is something I love about the language. However, when dealing with JSON blobs, CBOR blobs, and trees where nodes can be in many states, it adds a lot of code overhead, even if it'll optimize down after compilation.
3. It is ok to focus on small piece of the problem. Rust excels at a specific set of problems, and makes things such as parallelism much safer and easier. Leveraging these strengths made for a fast implementation of the initial build of a large HAMT.
4. Performance has many pitfalls, especially on modern hardware. Even though the algorithm and O(n) complexity of all the methods are at their core the same, there are big differences in speed due to the order of steps performed. Knowing how the hardware works is extremely important!