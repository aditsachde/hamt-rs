# hamt-rs
Hash array mapped trie for IPLD

These are my notes on the problem and optimzations. They have not been implemented here yet, rather they come from various testing I've done. There's probably things that I've missed and things will certainly change as I implement everything into this library. These notes are also a bit rambly and might be hard to visualize (hopefully will get around to adding diagrams) 

The goal is to build a very large (> 200 million KVs) Hash Mapped Array Trie. The details of the tree itself does not matter much. For the purposes of this document, we can model it as a simple binary tree that can store more than 1 element within a node, say similar to a 2-4 tree.

There are two main challenges. The first is that we do not want to assume that the entire tree can fit in memory. This may be the case large trees, also helps for the second constraint. If there is already a very large tree, we should be able to efficiently update it without loading the entire tree.

Originally, this was implemented in JS, which was way too slow. This rust port here is also slower than I'd like, which is why I've spent time figuring out optimization strategies. Its actually pretty fine for trees of reasonable sizes, but for tens of millions entries, it takes a while and I'm impatient 😉.

## Pointer chasing

Originally, the following structs and enums were used to model the tree. (All the code here is rust pseudocode)

```rust
pub struct Node<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> {
    pub map: BitVec<Lsb0, u8>,
    pub data: Vec<Element<V>>,
}

pub enum Element<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> {
    HashMapNode(HashMapNode<V>),
    Bucket(Vec<BucketEntry<V>>),
}

struct BucketEntry<V: Serialize + Debug + Encode<DagCborCodec> + Decode<DagCborCodec>> {
    key: Vec<u8>,
    value: V,
}
```

A node has a bitmap, from the `bitvec` crate, and a vector of elements. Each element can either be a pointer to another node, or a bucket, which is a vector of bucket entries, which itself contains a byte vector. 

This is a bad design as it leads to a lot of pointer chasing. When inserting an element into the tree, the tree must be traversed. This means that some number of pointer lookups are required. However, we should minimize this. When a node is going to be traversed to find a child node, there are three lookups that must be done. The location of the map, the location of the datavector, and then finally the child node. 

With the MVP stabilization of const generics in rust, we can do a bit better.

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

There are a couple differences. First, there is a constant generic parameter, which allows us to use arrays of a specific size within the struct. This does result in some overhead in the event that there are less than N data elements. Additionally a boolean array uses N bytes, instead of a bitvec, which could potentially use less. This was why vectors were orignally chosen. However, in practice, this this not the case. For most of the inner nodes in a HAMT, there will be N data elements, and the closer we get to the root, the more nodes will have N data elements. This means that there is actually very little benefit. Outer nodes use more memory, but this does not matter too much, as discussed later.

The element may also be a bucket, which is a vector, and the key within a bucket entry also still a vector. However, this matters less. If we are simply traversing a node, we will be able to get the pointer of the child node without any pointer lookups. If we do hit a bucket, it is the leaf node we want to insert a element into. 

## On disk storage

Next, lets take a look at how this tree will be serialized to disk in the context of IPLD. In this case, instead of using pointers, we use CIDs, which can simply be thought of as a hash of the serialized representation of a node.

CIDs are extremely cool, they let us uniquely identify an entire structure with only the CID of the root element. However, that is also a drawback. If we update a leaf node, its CID will change, and so will the CID of every parent node up to the root. We want to avoid doing this while building the tree. Calculating the CID takes a lot of time and the result will be thrown away almost immediately once the next element is inserted. 

As such, we need another identifier scheme for an intermediate tree. We can convert it to CIDs once we're completely done. This lets us avoid calculating unnecessary CIDs and also paralleize the work. Above, we use pointers, but this doesn't work if we want to serialize this intermediate state to disk. Instead lets use a simple integer as an identifier for each node, alongside a KV store such as `sled` to store nodes on disk.

```rust
pub enum Element<V, const N: usize> {
    HashMapNode(HashMapNodePointer),
    Bucket(Vec<BucketEntry<V>>),
}

HashMapNodePointer {
    Cid(cid),
    Id(usize)
}
```

If there is an existing tree we want to insert into, we can load nodes as neededd into our intermediate tree, assigning them integer ids. If a node is not needed for the specific operation, it is left as a cid.

## Memory caching 

This does work and at the same time brings our memory usage down to almost nothing, as the entire data structure is stored on disk. We still have memory though that we'd like to utilize, so lets add an LRU cache. The `lru` crate provides an implementation that has an API similar to a hashmap or a KV store. This makes things much simpler, as we can simply use the same integer identifier as we use for the ondisk KV store.

First, check the LRU cache to see if the node is in memory. If it is, use it. Otherwise, fetch the node from disk and insert it into the LRU cache for next time. If another node is evicted from cache, write it to disk.

This last portion is why pointers are not used for the in memory representation. If a node is evicted from cache and pointers are being used, the parent node must be updated, but the node itself does not contain a reference to its parent. There is likely some scheme to make this work with pointers, which would be incredibly useful.

## Cache Utilization

Can we estimate or improve cache utilization? Visualize a 7 node full binary tree. We have a cache big enough for the 3 inner nodes, plus one more. What is the best way to insert 8 more nodes? First, load node 4 into cache, then insert its two children. Then evict node 4 and load node 5. Insert its two children and so on and so forth.

A nice property of a HAMT is that it hashes keys to provide a better distribution. However, this annihilates cache hits. If we insert based on key orderings, we end up loading nodes from disk on every insert. Therefore we should pregenerate the hash and insert based on hash ordering, rather than key ordering. Luckily, there is an easy solution. Use `rayon` to generate hashes on multiple cores and insert them into `sled` (its lock free!), with the hash as keys. Then, iterate over the sled KV store, which is ordered by key. 

Not only does this completely fix our caching issue, but moves hashing the keys to the beginning and allows it to be parallelized, which would otherwise be done in the hot path later on. If we are guaranteed to insert keys ordered by their hash into an empty tree, there are also some more optimizations that can be done. However, these constraints don't make much sense for a library. 
