use crate::Backend;

#[derive(Default, Debug)]
pub struct BackendPool {
    // private
    items: Vec<Backend>,
}

impl BackendPool {
    pub fn new() -> Self {
        Self { items: vec![] }
    }

    pub fn add(&mut self, backend: Backend) {
        self.items.push(backend);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Backend> {
        self.items.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Backend> {
        self.items.get_mut(index)
    }
}
