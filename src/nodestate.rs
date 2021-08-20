pub trait HashMapState {}

#[derive(Debug)]
pub struct Valid;
impl HashMapState for Valid {}

#[derive(Debug)]
pub struct Edit;
impl HashMapState for Edit {}

#[derive(Debug)]
pub struct Complete;
impl HashMapState for Complete {}
