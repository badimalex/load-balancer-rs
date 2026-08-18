use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use simpleload_balancer_rs::{
    Backend, BackendPool, LoadBalancer, http_server::serve, http_server::start_health_checker,
};
use tokio::{net::TcpListener, sync::oneshot::Sender};
type HttpClient = Client<HttpConnector, Full<Bytes>>;
use hyper_util::rt::TokioExecutor;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use std::time::Duration;
use tokio::time::sleep;

async fn spawn_proxy(
    load_balancer: LoadBalancer,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shared_load_balancer = Arc::new(Mutex::new(load_balancer));

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    start_health_checker(
        Arc::clone(&shared_load_balancer),
        client.clone(),
        Duration::from_millis(100),
    );

    let handle = tokio::spawn(async move {
        serve(listener, shared_load_balancer, client).await.unwrap();
    });

    (addr, handle)
}

#[tokio::test]
async fn test_empty_pool_returns_503() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let pool = BackendPool::new();
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let shared_load_balancer = Arc::new(Mutex::new(LoadBalancer::new(pool)));

    let server_handle = tokio::spawn(serve(listener, shared_load_balancer, client.clone()));
    let req = Request::builder()
        .uri(format!("http://{}/any", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = res.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"no backends available");

    server_handle.abort();
}

async fn spawn_backend(body: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    address
}

async fn spawn_backend_on_addr(body: &'static str, addr: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind(addr).await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    address
}

#[tokio::test]
async fn test_health_does_not_advance_round_robin() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let mut pool = BackendPool::new();
    let backend_a = spawn_backend("backend-a").await;
    let backend_b = spawn_backend("backend-b").await;

    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(Backend::new(format!("http://{backend_b}")));

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let shared_load_balancer = Arc::new(Mutex::new(LoadBalancer::new(pool)));

    let server_handle = tokio::spawn(serve(listener, shared_load_balancer, client.clone()));

    // Запрос /health не должен сдвигать балансировку
    let req_health = Request::builder()
        .method("GET")
        .uri(format!("http://{}/health", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();
    let _ = client.request(req_health).await.unwrap();

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::OK);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    server_handle.abort();
}

#[tokio::test]
async fn http_routing_skips_unhealthy_backend() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let mut pool = BackendPool::new();
    let backend_a = spawn_backend("backend-a").await;
    let backend_b = spawn_backend("backend-b").await;
    let backend_c = spawn_backend("backend-c").await;

    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(Backend::new(format!("http://{backend_b}")));
    pool.add(Backend::new(format!("http://{backend_c}")));

    let mut load_balancer = LoadBalancer::new(pool);
    load_balancer.set_backend_healthy(1, false);
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let shared_load_balancer = Arc::new(Mutex::new(load_balancer));

    let server_handle = tokio::spawn(serve(listener, shared_load_balancer, client.clone()));

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::OK);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    let req2 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res2 = client.request(req2).await.unwrap();

    assert_eq!(res2.status(), StatusCode::OK);
    let body = res2.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-c");

    let req3 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res3 = client.request(req3).await.unwrap();

    assert_eq!(res3.status(), StatusCode::OK);
    let body = res3.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    let req4 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res4 = client.request(req4).await.unwrap();

    assert_eq!(res4.status(), StatusCode::OK);
    let body = res4.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-c");

    server_handle.abort();
}

#[tokio::test]
async fn upstream_failure_marks_backend_unhealthy() {
    let mut pool = BackendPool::new();
    let backend_a = spawn_backend("backend-a").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_addr = listener.local_addr().unwrap();
    drop(listener);

    let backend_b = Backend::new(format!("http://{unavailable_addr}"));

    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(backend_b);

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;

    // req 1

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::OK);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // req 2

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::BAD_GATEWAY);
    let body = res1.collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"bad gateway");

    // req 3

    let req3 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res3 = client.request(req3).await.unwrap();

    assert_eq!(res3.status(), StatusCode::OK);
    let body = res3.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // req 4

    let req4 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res4 = client.request(req4).await.unwrap();

    assert_eq!(res4.status(), StatusCode::OK);
    let body = res4.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    handle.abort();
}

#[tokio::test]
async fn recovered_backend_returns_to_http_routing() {
    let mut pool = BackendPool::new();
    let backend_a = spawn_backend("backend-a").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_addr = listener.local_addr().unwrap();
    drop(listener);

    let backend_b = Backend::new(format!("http://{unavailable_addr}"));

    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(backend_b);

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;

    // req 1

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::OK);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // req 2

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::BAD_GATEWAY);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"bad gateway");

    // req 3

    let req3 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res3 = client.request(req3).await.unwrap();

    assert_eq!(res3.status(), StatusCode::OK);
    let body = res3.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    let _ = spawn_backend_on_addr("backend-b", unavailable_addr).await;

    tokio::time::sleep(Duration::from_millis(300)).await;
    // req 4

    let req4 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res4 = client.request(req4).await.unwrap();

    assert_eq!(res4.status(), StatusCode::OK);
    let body = res4.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-b");

    handle.abort();
}

#[tokio::test]
async fn backend_remains_unhealthy_if_not_recovered() {
    let mut pool = BackendPool::new();
    let backend_a = spawn_backend("backend-a").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_addr = listener.local_addr().unwrap();
    drop(listener);

    let backend_b = Backend::new(format!("http://{unavailable_addr}"));

    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(backend_b);

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;

    // req 1

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::OK);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // req 2

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::BAD_GATEWAY);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"bad gateway");

    tokio::time::sleep(Duration::from_millis(900)).await;
    // req 3

    let req3 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res3 = client.request(req3).await.unwrap();

    assert_eq!(res3.status(), StatusCode::OK);
    let body = res3.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // req 4

    let req4 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res4 = client.request(req4).await.unwrap();

    assert_eq!(res4.status(), StatusCode::OK);
    let body = res4.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    handle.abort();
}

#[tokio::test]
async fn multiple_unhealthy_backends_recover_independently() {
    let mut pool = BackendPool::new();
    let backend_a = spawn_backend("backend-a").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_addr = listener.local_addr().unwrap();
    drop(listener);

    let listener_c = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_addr_c = listener_c.local_addr().unwrap();
    drop(listener_c);

    let backend_b = Backend::new(format!("http://{unavailable_addr}"));
    let backend_c = Backend::new(format!("http://{unavailable_addr_c}"));

    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(backend_b);
    pool.add(backend_c);

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;

    // req 1

    let req1 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res1 = client.request(req1).await.unwrap();

    assert_eq!(res1.status(), StatusCode::OK);
    let body = res1.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // req 2

    let req2 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res2 = client.request(req2).await.unwrap();

    assert_eq!(res2.status(), StatusCode::BAD_GATEWAY);
    let body = res2.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"bad gateway");

    // req 3

    let req3 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res3 = client.request(req3).await.unwrap();

    assert_eq!(res3.status(), StatusCode::BAD_GATEWAY);
    let body = res3.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"bad gateway");

    // req 4

    let req4 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res4 = client.request(req4).await.unwrap();

    assert_eq!(res4.status(), StatusCode::OK);
    let body = res4.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // res 5

    let req5 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res5 = client.request(req5).await.unwrap();

    assert_eq!(res5.status(), StatusCode::OK);
    let body = res5.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    let _ = spawn_backend_on_addr("backend-b", unavailable_addr).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    // req 6

    let req6 = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res6 = client.request(req6).await.unwrap();

    assert_eq!(res6.status(), StatusCode::OK);
    let body = res6.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-b");

    // req 7

    let req = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res = client.request(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = res.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-a");

    // req 8

    let req = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res = client.request(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = res.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-b");

    let _ = spawn_backend_on_addr("backend-c", unavailable_addr_c).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    // req 9

    let req = Request::builder()
        .method("GET")
        .uri(format!("http://{}/", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res = client.request(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = res.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"backend-c");

    handle.abort();
}

#[tokio::test]
async fn test_health_check() {
    let pool = BackendPool::new();
    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .method("GET")
        .uri(format!("http://{}/health", addr))
        .header("Host", addr.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();

    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = res.collect().await.unwrap().to_bytes();
    assert_eq!(&body_bytes[..], b"ok");

    handle.abort();
}

#[tokio::test]
async fn test_request_forwarded_to_backend() {
    let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_addr = backend_listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = backend_listener.accept().await {
            let response = "HTTP/1.1 201 Created\r\nContent-Length: 16\r\nConnection: close\r\n\r\nbackend-response";
            use tokio::io::AsyncWriteExt;
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    let mut pool = BackendPool::new();
    let backend1 = Backend::new(format!("http://{}", backend_addr));

    pool.add(backend1);

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let body = res.collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"backend-response");

    handle.abort();
}

#[tokio::test]
async fn request_path_and_query_are_forwarded() {
    let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_addr = backend_listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = backend_listener.accept().await {
            let mut reader = BufReader::new(stream);
            let mut request_line = String::new();

            if reader.read_line(&mut request_line).await.is_ok() {
                let parts: Vec<&str> = request_line.split_whitespace().collect();
                let target = parts.get(1).unwrap_or(&"/");

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    target.len(),
                    target
                );

                reader
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .unwrap();
            }
        }
    });

    let mut pool = BackendPool::new();
    let backend1 = Backend::new(format!("http://{}", backend_addr));

    pool.add(backend1);

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .uri(format!("http://{}/users?id=15&active=true", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();

    let body = res.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"/users?id=15&active=true");

    handle.abort();
}

#[tokio::test]
async fn post_method_and_body_are_forwarded() {
    let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_addr = backend_listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = backend_listener.accept().await {
            use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            if reader.read_line(&mut line).await.is_err() {
                continue;
            }
            let p: Vec<&str> = line.split_whitespace().collect();
            let (m, u) = (p.first().unwrap_or(&""), p.get(1).unwrap_or(&"/"));

            let mut len = 0;
            loop {
                let mut l = String::new();
                if reader.read_line(&mut l).await.is_ok()
                    && (l == "\r\n" || l == "\n" || l.is_empty())
                {
                    break;
                }
                if let Some(rest) = l.to_lowercase().strip_prefix("content-length:") {
                    len = rest.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            let _ = reader.read_exact(&mut body).await;
            let b_str = String::from_utf8_lossy(&body);
            let resp = format!("{m} {u} {b_str}");
            let res = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp.len(),
                resp
            );
            let _ = stream.write_all(res.as_bytes()).await;
        }
    });

    let mut pool = BackendPool::new();
    let backend1 = Backend::new(format!("http://{}", backend_addr));

    pool.add(backend1);

    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .method("POST")
        .uri(format!("http://{}/orders", addr))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Full::new(Bytes::from("product=book&count=2")))
        .unwrap();

    let res = client.request(req).await.unwrap();

    let body = res.collect().await.unwrap().to_bytes();

    assert_eq!(&body[..], b"POST /orders product=book&count=2");

    handle.abort();
}

#[tokio::test]
async fn test_unreachable_backend_returns_502() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable_addr = listener.local_addr().unwrap();
    drop(listener);
    let backend = Backend::new(format!("http://{unavailable_addr}"));

    let mut pool = BackendPool::new();

    pool.add(backend);
    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);

    let body2 = res.collect().await.unwrap().to_bytes();
    assert_eq!(&body2[..], b"bad gateway");

    handle.abort();
}

#[tokio::test]
async fn invalid_backend_uri_returns_502_without_panic() {
    let backend = Backend::new("http://[::1".to_string());

    let mut pool = BackendPool::new();

    pool.add(backend);
    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);

    let body2 = res.collect().await.unwrap().to_bytes();
    assert_eq!(&body2[..], b"bad gateway");

    handle.abort();
}

async fn spawn_slow_backend(delay: Duration, body: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            sleep(delay).await;

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    address
}

async fn spawn_slow_backend_with_confirm(
    delay: Duration,
    body: &'static str,
    slow_started_tx: Sender<()>,
) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            slow_started_tx.send(()).unwrap();
            sleep(delay).await;

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });

    address
}

#[tokio::test]
async fn slow_backend_returns_504() {
    let mut pool = BackendPool::new();
    let delay = Duration::from_secs(3);
    let backend_a = spawn_slow_backend(delay, "backend-a").await;

    pool.add(Backend::new(format!("http://{backend_a}")));
    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;
    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let req = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let res = client.request(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::GATEWAY_TIMEOUT);

    let body = res.collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"gateway timeout");

    handle.abort();
}

#[tokio::test]
async fn fast_request_is_not_blocked_by_slow_request() {
    let (slow_started_tx, slow_started_rx) = tokio::sync::oneshot::channel();
    let mut pool = BackendPool::new();
    let delay = Duration::from_millis(900);

    // 1. Запуск backend на случайных портах (медленный, затем быстрый)
    let backend_a = spawn_slow_backend_with_confirm(delay, "backend-a", slow_started_tx).await;
    let backend_b = spawn_backend("backend-b").await;

    // 2. Добавление в пул: медленный (a), затем быстрый (b)
    pool.add(Backend::new(format!("http://{backend_a}")));
    pool.add(Backend::new(format!("http://{backend_b}")));

    // 3. Запуск proxy, чтобы Round Robin начинался с первого бэкенда
    let (addr, handle) = spawn_proxy(LoadBalancer::new(pool)).await;

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    // Формирование первого запроса
    let req1 = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();

    // 4. Отправка первого запроса
    let client_clone1 = client.clone();
    let mut handle1 = tokio::spawn(async move { client_clone1.request(req1).await.unwrap() });

    // Убедимся, что медленный бэкенд успел принять запрос
    slow_started_rx.await.unwrap();

    // 5. Не дожидаясь ответа первого, отправляем второй запрос
    let req2 = Request::builder()
        .uri(format!("http://{}/", addr))
        .body(Full::new(Bytes::new()))
        .unwrap();

    let client_clone2 = client.clone();
    let mut handle2 = tokio::spawn(async move { client_clone2.request(req2).await.unwrap() });

    let (win, los) = tokio::select! {
        res1 = &mut handle1 => (res1.unwrap(), handle2),
        res2 = &mut handle2 => (res2.unwrap(), handle1),
    };

    assert_ne!(win.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(win.status(), StatusCode::OK);
    let body2 = win.collect().await.unwrap().to_bytes();
    assert_eq!(&body2[..], b"backend-b");

    let los_res = los.await.unwrap();
    assert_ne!(los_res.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(los_res.status(), StatusCode::OK);
    let body1 = los_res.collect().await.unwrap().to_bytes();
    assert_eq!(&body1[..], b"backend-a");

    handle.abort();
}
