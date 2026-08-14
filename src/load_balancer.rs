use crate::{BackendPool, RoundRobin};

#[derive(Default, Debug)]
pub struct LoadBalancer {
    // private
    round_robin: RoundRobin,
    backend_pool: BackendPool,
}

impl LoadBalancer {
    pub fn new(backend_pool: BackendPool) -> Self {
        Self {
            backend_pool,
            round_robin: RoundRobin::new(),
        }
    }

    pub fn route(&mut self) -> Option<&str> {
        let index = self.round_robin.select_next(&self.backend_pool)?;
        let backend = self.backend_pool.get(index)?;
        Some(backend.address())
    }
}
