use std::collections::HashMap;
use cid::Cid;

pub trait CidStore {
    fn new() -> Self;
    fn get(&self, cid: &Cid) -> Option<&Vec<u8>>;
    fn insert(&mut self, cid: &Cid, block: Vec<u8>) -> Option<Vec<u8>>;
    fn remove(&mut self, cid: &Cid) -> Option<Vec<u8>>;
}

#[derive(Debug)]
pub struct MemStore {
    hashmap: HashMap<Cid, Vec<u8>>
}

impl CidStore for MemStore {
    fn new() -> Self {
        Self {
            hashmap: HashMap::new()
        }
    }

    fn get(&self, cid: &Cid) -> Option<&Vec<u8>> {
        self.hashmap.get(cid)
    }

    fn insert(&mut self, cid: &Cid, block: Vec<u8>) -> Option<Vec<u8>> {
        self.hashmap.insert(cid.clone(), block)
    }

    fn remove(&mut self, cid: &Cid) -> Option<Vec<u8>> {
        self.hashmap.remove(cid)
    }
}