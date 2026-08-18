use crate::BackendPool;

#[derive(Default, Debug)]
pub struct LoadBalancer {
    current: usize,
    backend_pool: BackendPool,
}

#[derive(Debug, Clone)]
pub struct BackendSnapshot {
    pub index: usize,
    pub address: String,
}

impl LoadBalancer {
    pub fn new(backend_pool: BackendPool) -> Self {
        Self {
            backend_pool,
            current: 0,
        }
    }

    pub fn set_backend_healthy(&mut self, index: usize, healthy: bool) {
        if let Some(current_backend) = self.backend_pool.get_mut(index) {
            current_backend.set_healthy(healthy);
        }
    }

    pub fn route(&mut self) -> Option<(usize, &str)> {
        let len = self.backend_pool.len();
        if len == 0 {
            return None;
        }

        for _ in 0..len {
            let idx = self.current;
            self.current = (self.current + 1) % len;

            if let Some(backend) = self.backend_pool.get(idx)
                && backend.is_healthy()
            {
                return Some((idx, backend.address()));
            }
        }

        None
    }

    pub fn unhealthy_backends_snapshot(&self) -> Vec<BackendSnapshot> {
        let len = self.backend_pool.len();
        let mut snapshot = Vec::with_capacity(len);

        for idx in 0..len {
            if let Some(backend) = self.backend_pool.get(idx)
                && !backend.is_healthy()
            {
                snapshot.push(BackendSnapshot {
                    index: idx,
                    address: backend.address().to_string(),
                });
            }
        }

        snapshot
    }
}
