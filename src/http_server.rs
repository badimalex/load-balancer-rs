use crate::AppError;
use crate::LoadBalancer;

use bytes::Bytes;
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::header::HOST;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, StatusCode};
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use tracing::{error, info, warn};

type HttpClient = Client<HttpConnector, Full<Bytes>>;

pub async fn run(
    address: &str,
    load_balancer: LoadBalancer,
    upstream_timeout_ms: u64,
    health_check_interval_ms: u64,
    cancellation: CancellationToken,
) -> Result<(), AppError> {
    let listener = TcpListener::bind(address).await.map_err(AppError::Bind)?;

    let client: HttpClient = Client::builder(TokioExecutor::new()).build(HttpConnector::new());
    let shared_load_balancer = Arc::new(Mutex::new(load_balancer));

    // Сигнал ОС на завершение (Ctrl+C)
    let signal_token = cancellation.clone();
    let signal_handle = tokio::spawn(async move {
        tokio::select! {
              sig = tokio::signal::ctrl_c() => {
                    match sig {
                         Ok(_) => {
                            info!("Shutdown started");
                            signal_token.cancel();
                            Ok(())
                        }
                        Err(e) => {
                            signal_token.cancel();
                            Err(AppError::Shutdown(e))
                        }
                    }
                }
            _ = signal_token.cancelled() => {
                // приложение завершилось по другой причине
                Ok(())
            }
        }
    });

    let mut serve_handle = tokio::spawn(serve(
        listener,
        Arc::clone(&shared_load_balancer),
        client.clone(),
        upstream_timeout_ms,
        cancellation.clone(),
    ));

    let mut health_handle = tokio::spawn(start_health_checker(
        shared_load_balancer,
        client,
        Duration::from_millis(health_check_interval_ms),
        cancellation.clone(),
    ));

    let (winner, loser) = tokio::select! {
        res = &mut serve_handle => (res, health_handle),
        res = &mut health_handle => (res, serve_handle),
    };

    cancellation.cancel();

    let loser_result = loser.await;
    signal_handle.await.map_err(AppError::Task)??;

    winner.map_err(AppError::Task)??;
    loser_result.map_err(AppError::Task)??;

    info!("Shutdown completed");
    Ok(())
}

async fn handle_request(
    request: Request<Incoming>,
    load_balancer_obj: Arc<Mutex<LoadBalancer>>,
    client: HttpClient,
    upstream_timeout_ms: u64,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = request.method();
    let path = request.uri().path();

    match (method, path) {
        (&Method::GET, "/health") => Ok(text_response(StatusCode::OK, "ok")),

        _ => {
            let (backend_address, backend_index) = {
                let Ok(mut guard) = load_balancer_obj.lock() else {
                    return Ok(text_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal server error",
                    ));
                };

                let Some((index, address)) = guard.route() else {
                    return Ok(text_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "no backends available",
                    ));
                };

                (address.to_owned(), index)
            };

            let path_and_query = request
                .uri()
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/");
            let full_url = format!("{}{}", backend_address, path_and_query);

            let uri: hyper::Uri = match full_url.parse() {
                Ok(uri) => uri,
                Err(_) => {
                    return Ok(text_response(StatusCode::BAD_GATEWAY, "bad gateway"));
                }
            };

            let (mut parts, incoming_body) = request.into_parts();
            parts.uri = uri;
            parts.headers.remove(HOST);

            let collected = match incoming_body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    return Ok(text_response(
                        StatusCode::BAD_REQUEST,
                        "Failed to read the request body",
                    ));
                }
            };

            let full_body = Full::new(collected);

            let upstream_result = timeout(Duration::from_millis(upstream_timeout_ms), async {
                let new_req = Request::from_parts(parts, full_body);
                let res = client.request(new_req).await?;
                let (parts, body) = res.into_parts();
                let bytes = body.collect().await?.to_bytes();

                Ok::<_, Box<dyn std::error::Error>>(Response::from_parts(parts, bytes.into()))
            })
            .await;

            match upstream_result {
                Ok(Ok(response)) => Ok(response),
                /* обработка ошибки запроса */
                Ok(Err(_)) => {
                    if let Ok(mut guard) = load_balancer_obj.lock() {
                        guard.set_backend_healthy(backend_index, false);
                        warn!(backend_url = %backend_address, "Backend marked unhealthy");
                    }
                    Ok(text_response(StatusCode::BAD_GATEWAY, "bad gateway"))
                }
                /* обработка истечения времени (Elapsed) */
                Err(_) => Ok(text_response(
                    StatusCode::GATEWAY_TIMEOUT,
                    "gateway timeout",
                )),
            }
        }
    }
}

fn text_response(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

async fn check_backend_health(client: HttpClient, backend_address: &str) -> bool {
    let req = Request::builder()
        .method("GET")
        .uri(backend_address)
        .body(Full::new(Bytes::from("")))
        .unwrap();

    match tokio::time::timeout(Duration::from_secs(2), client.request(req)).await {
        Ok(Ok(res)) => res.status() == StatusCode::OK,
        _ => false,
    }
}

pub async fn serve(
    listener: TcpListener,
    load_balancer: Arc<Mutex<LoadBalancer>>,
    client: HttpClient,
    upstream_timeout_ms: u64,
    shutdown_token: CancellationToken,
) -> Result<(), AppError> {
    let mut tasks = JoinSet::new();

    loop {
        let client_for_connection = client.clone();

        tokio::select! {
            Some(res) = tasks.join_next(), if !tasks.is_empty() => {
                match res {
                    Ok(()) => {}
                    Err(join_err) => return Err(AppError::Task(join_err)),
                }
            }

            result = listener.accept() => {
                let (stream, _) = result.map_err(AppError::Accept)?;

                if shutdown_token.is_cancelled() {
                    break;
                }

                let proxy_token = shutdown_token.clone();
                let io = TokioIo::new(stream);
                let load_balancer_clone = Arc::clone(&load_balancer);

                tasks.spawn(async move {
                    let service = service_fn(move |req| {
                        let data_clone = Arc::clone(&load_balancer_clone);
                        let client_for_request = client_for_connection.clone();
                        async move {
                            handle_request(req, data_clone, client_for_request, upstream_timeout_ms)
                                .await
                        }
                    });

                    tokio::select! {
                        res = http1::Builder::new().serve_connection(io, service) => {
                            if let Err(err) = res {
                                error!(error = %err, "Upstream connection error");
                            }
                        }
                        _ = proxy_token.cancelled() => {
                        }
                    }
                });
            }

            _ = shutdown_token.cancelled() => {
                break;
            }
        }
    }

    while let Some(res) = tasks.join_next().await {
        match res {
            // Успешное завершение таски -> переходим к следующей
            Ok(()) => continue,

            // Ошибка (паника или отмена) -> прерываемся и возвращаем AppError
            Err(join_err) => return Err(AppError::Task(join_err)),
        }
    }

    Ok(())
}

pub async fn start_health_checker(
    load_balancer: Arc<Mutex<LoadBalancer>>,
    client: HttpClient,
    interval: Duration,
    cancellation: CancellationToken,
) -> Result<(), AppError> {
    let mut tasks = JoinSet::new();

    loop {
        tokio::select! {
            Some(res) = tasks.join_next(), if !tasks.is_empty() => {
                match res {
                    Ok(()) => {}
                    Err(join_err) => return Err(AppError::Task(join_err)),
                }
            }

            _ = cancellation.cancelled() => {
                break;
            }
            _ = sleep(interval) => {
                let backends_snapshot = {
                    match load_balancer.lock() {
                        Ok(guard) => guard.unhealthy_backends_snapshot(),
                        Err(_) => continue,
                    }
                };
                for backend in backends_snapshot {
                    let cancellation_clone = cancellation.clone();
                    let client_clone = client.clone();
                    let lb_clone = Arc::clone(&load_balancer);
                    tasks.spawn(async move {
                        let result = tokio::select! {
                            _ = cancellation_clone.cancelled() => {
                                return;
                            }

                            result = check_backend_health(client_clone, &backend.address) => {
                                result
                            }
                        };

                        if let Ok(mut guard) = lb_clone.lock() {
                            guard.set_backend_healthy(backend.index, result);
                        }
                    });
                }
            }
        }
    }

    while let Some(res) = tasks.join_next().await {
        match res {
            // Успешное завершение таски -> переходим к следующей
            Ok(()) => continue,

            // Ошибка (паника или отмена) -> прерываемся и возвращаем AppError
            Err(join_err) => return Err(AppError::Task(join_err)),
        }
    }

    Ok(())
}
