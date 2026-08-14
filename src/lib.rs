pub mod backend;
pub mod http_server;

mod backend_pool;
mod load_balancer;
mod routing;

pub use backend::Backend;
pub use backend_pool::BackendPool;
pub use load_balancer::LoadBalancer;
pub use routing::round_robin::RoundRobin;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_exposes_its_address() {
        let backend = Backend::new("server-a:8080".to_string());

        assert_eq!(backend.address(), "server-a:8080");
    }

    #[test]
    fn new_pool_is_empty() {
        let pool = BackendPool::new();

        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());

        let backend = pool.get(0);
        assert!(backend.is_none());
    }

    #[test]
    fn add_updates_pool_len_and_empty_state() {
        let mut pool = BackendPool::new();
        let backend = Backend::new("server-a:8080".to_string());

        pool.add(backend);

        assert_eq!(pool.len(), 1);
        assert!(!pool.is_empty());
    }

    #[test]
    fn get_returns_backend_by_valid_index() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));

        let backend = pool.get(0).expect("backend at index 0 should exist");

        assert_eq!(backend.address(), "server-a:8080");
    }

    #[test]
    fn get_returns_none_for_out_of_bounds_index() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));

        let backend = pool.get(1);

        assert!(backend.is_none());
    }

    #[test]
    fn pool_preserves_insertion_order() {
        let mut pool = BackendPool::new();

        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        pool.add(Backend::new("server-c:8080".to_string()));

        assert_eq!(pool.get(0).unwrap().address(), "server-a:8080");
        assert_eq!(pool.get(1).unwrap().address(), "server-b:8080");
        assert_eq!(pool.get(2).unwrap().address(), "server-c:8080");
    }

    // RoundRobin

    #[test]
    fn selection_wraps_after_last_backend() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        pool.add(Backend::new("server-c:8080".to_string()));
        pool.add(Backend::new("server-d:8080".to_string()));

        let mut selector = RoundRobin::new();

        assert_eq!(selector.select_next(&pool), Some(0));
        assert_eq!(selector.select_next(&pool), Some(1));
        assert_eq!(selector.select_next(&pool), Some(2));
        assert_eq!(selector.select_next(&pool), Some(3));
        assert_eq!(selector.select_next(&pool), Some(0));
        assert_eq!(selector.select_next(&pool), Some(1));
        assert_eq!(selector.select_next(&pool), Some(2));
        assert_eq!(selector.select_next(&pool), Some(3));
    }

    #[test]
    fn single_backend_is_selected_repeatedly() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));

        let mut selector = RoundRobin::new();

        for _ in 0..5 {
            assert_eq!(selector.select_next(&pool), Some(0));
        }
    }

    #[test]
    fn round_robin_instances_have_independent_state() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        pool.add(Backend::new("server-c:8080".to_string()));

        let mut first_selector = RoundRobin::new();
        let mut second_selector = RoundRobin::new();

        assert_eq!(first_selector.select_next(&pool), Some(0));
        assert_eq!(first_selector.select_next(&pool), Some(1));

        assert_eq!(second_selector.select_next(&pool), Some(0));
        assert_eq!(first_selector.select_next(&pool), Some(2));
        assert_eq!(second_selector.select_next(&pool), Some(1));
    }

    #[test]
    fn empty_pool_returns_none() {
        let pool = BackendPool::new();
        let mut selector = RoundRobin::new();

        assert_eq!(selector.select_next(&pool), None);
    }

    #[test]
    fn first_selection_returns_zero() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));

        let mut selector = RoundRobin::new();

        assert_eq!(selector.select_next(&pool), Some(0));
    }

    #[test]
    fn successive_selections_follow_pool_order() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        pool.add(Backend::new("server-c:8080".to_string()));

        let mut selector = RoundRobin::new();

        assert_eq!(selector.select_next(&pool), Some(0));
        assert_eq!(selector.select_next(&pool), Some(1));
        assert_eq!(selector.select_next(&pool), Some(2));
    }

    #[test]
    fn round_robin_selection_resolves_to_backend_addresses() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        pool.add(Backend::new("server-c:8080".to_string()));

        let mut selector = RoundRobin::new();

        let expected_addresses = [
            "server-a:8080",
            "server-b:8080",
            "server-c:8080",
            "server-a:8080",
        ];

        for expected_address in expected_addresses {
            let index = selector
                .select_next(&pool)
                .expect("non-empty pool should produce an index");

            let backend = pool
                .get(index)
                .expect("selector should produce a valid pool index");

            assert_eq!(backend.address(), expected_address);
        }
    }

    #[test]
    fn empty_pool_does_not_advance_selector_state() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));

        let empty_pool = BackendPool::new();
        let mut selector = RoundRobin::new();

        assert_eq!(selector.select_next(&pool), Some(0));
        assert_eq!(selector.select_next(&empty_pool), None);
        assert_eq!(selector.select_next(&pool), Some(1));
    }

    // LoadBalancer
    #[test]
    fn test_empty_load_balancer() {
        let pool = BackendPool::new();
        let mut load_balancer = LoadBalancer::new(pool);
        assert_eq!(load_balancer.route(), None);
        assert_eq!(load_balancer.route(), None);
    }

    #[test]
    fn test_single_backend() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        let mut load_balancer = LoadBalancer::new(pool);

        for _ in 0..5 {
            assert_eq!(load_balancer.route(), Some("server-a:8080"));
        }
    }

    #[test]
    fn test_round_robin_routing() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a:8080".to_string()));
        pool.add(Backend::new("server-b:8080".to_string()));
        let mut load_balancer = LoadBalancer::new(pool);

        assert_eq!(load_balancer.route(), Some("server-a:8080"));
        assert_eq!(load_balancer.route(), Some("server-b:8080"));
        assert_eq!(load_balancer.route(), Some("server-a:8080"));
    }

    #[test]
    fn test_three_backends_seven_calls() {
        let mut pool = BackendPool::new();
        pool.add(Backend::new("server-a".to_string()));
        pool.add(Backend::new("server-b".to_string()));
        pool.add(Backend::new("server-c".to_string()));
        let mut lb = LoadBalancer::new(pool);

        assert_eq!(lb.route(), Some("server-a"));
        assert_eq!(lb.route(), Some("server-b"));
        assert_eq!(lb.route(), Some("server-c"));
        assert_eq!(lb.route(), Some("server-a"));
        assert_eq!(lb.route(), Some("server-b"));
        assert_eq!(lb.route(), Some("server-c"));
        assert_eq!(lb.route(), Some("server-a"));
    }

    #[test]
    fn test_instances_independence() {
        let mut pool1 = BackendPool::new();
        pool1.add(Backend::new("a".to_string()));
        pool1.add(Backend::new("b".to_string()));
        pool1.add(Backend::new("c".to_string()));
        let mut lb1 = LoadBalancer::new(pool1);

        let mut pool2 = BackendPool::new();
        pool2.add(Backend::new("a".to_string()));
        pool2.add(Backend::new("b".to_string()));
        pool2.add(Backend::new("c".to_string()));
        let mut lb2 = LoadBalancer::new(pool2);

        assert_eq!(lb1.route(), Some("a"));
        assert_eq!(lb1.route(), Some("b"));

        assert_eq!(lb2.route(), Some("a"));

        assert_eq!(lb1.route(), Some("c"));
        assert_eq!(lb2.route(), Some("b"));
        assert_eq!(lb2.route(), Some("c"));
    }
}
