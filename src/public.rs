use crate::installer;
use crate::manager::get_plugin_runtime_manager;
use crate::registry;
use axum::{
    body::{to_bytes, Body},
    extract::{OriginalUri, Path, Request, ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade}},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
use url::Url;
use yh_config_infra::RequestContext;
use yh_response::{AppError, error::ErrorCode};

fn internal_error(ctx: &RequestContext, message: impl Into<String>) -> AppError {
    AppError::internal(message, Arc::clone(&ctx.request_id), Arc::clone(&ctx.client_ip))
}

fn blank_error(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::new(code, message, Arc::from(""), Arc::from(""))
}

fn safe_join(base: &StdPath, name: &str) -> Result<PathBuf, AppError> {
    let path = StdPath::new(name);
    if path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(blank_error(
            ErrorCode::BadRequest,
            format!("invalid relative path: {}", name),
        ));
    }
    Ok(base.join(path))
}

async fn load_package_root(
    db: &Arc<sea_orm::DatabaseConnection>,
    plugin_id: &str,
) -> Result<(PathBuf, crate::manifest::PluginManifest), AppError> {
    let plugin = registry::get_registry_by_id(db.as_ref(), plugin_id)
        .await
        .map_err(|e| blank_error(ErrorCode::InternalError, format!("failed to load plugin registry: {}", e)))?
        .ok_or_else(|| blank_error(ErrorCode::NotFound, format!("Plugin '{}' not found", plugin_id)))?;
    let version = plugin
        .current_version
        .as_ref()
        .ok_or_else(|| blank_error(ErrorCode::BadRequest, format!("Plugin '{}' has no installed version", plugin_id)))?;
    let version = registry::get_version_by_plugin_and_version(db.as_ref(), plugin_id, version)
        .await
        .map_err(|e| blank_error(ErrorCode::InternalError, format!("failed to load plugin version: {}", e)))?
        .ok_or_else(|| blank_error(ErrorCode::NotFound, format!("Plugin '{}' current version record not found", plugin_id)))?;
    let install_root = PathBuf::from(version.package_path);
    let manifest = installer::read_manifest_from_package_dir(&install_root)
        .await
        .map_err(|e| blank_error(ErrorCode::BadRequest, e))?;
    Ok((install_root, manifest))
}

async fn active_route_base_url(plugin_id: &str, ctx: &RequestContext) -> Result<String, AppError> {
    let manager = get_plugin_runtime_manager().ok_or_else(|| {
        AppError::internal(
            "plugin runtime manager is not initialized",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    let handle = manager.get_runtime_handle(plugin_id).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            format!("Plugin '{}' runtime is not active", plugin_id),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    handle.route_base_url.ok_or_else(|| {
        AppError::new(
            ErrorCode::BadRequest,
            "plugin runtime has no proxy base url",
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })
}

pub async fn serve_plugin_ui_root(
    axum::Extension(db): axum::Extension<Arc<sea_orm::DatabaseConnection>>,
    Path(plugin_id): Path<String>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    serve_plugin_ui_file(db, plugin_id, String::new(), ctx).await
}

pub async fn serve_plugin_ui(
    axum::Extension(db): axum::Extension<Arc<sea_orm::DatabaseConnection>>,
    Path((plugin_id, path)): Path<(String, String)>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<impl IntoResponse, AppError> {
    serve_plugin_ui_file(db, plugin_id, path, ctx).await
}

async fn serve_plugin_ui_file(
    db: Arc<sea_orm::DatabaseConnection>,
    plugin_id: String,
    path: String,
    ctx: RequestContext,
) -> Result<Response, AppError> {
    let (install_root, manifest) = load_package_root(&db, &plugin_id).await?;
    let Some(ui) = manifest.ui else {
        return Err(AppError::new(
            ErrorCode::NotFound,
            format!("Plugin '{}' does not provide a UI bundle", plugin_id),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        ));
    };
    let root = install_root.join("ui");
    let relative = if path.is_empty() {
        ui.root.clone()
    } else {
        format!("{}/{}", ui.root.trim_end_matches('/'), path)
    };
    let candidate = safe_join(&root, &relative)?;
    let selected = if tokio::fs::try_exists(&candidate)
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to check ui asset: {}", e)))?
    {
        candidate
    } else {
        root.join("index.html")
    };
    let bytes = tokio::fs::read(&selected)
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to read ui asset: {}", e)))?;
    let content_type = mime_guess::from_path(&selected).first_or_octet_stream().to_string();
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_str(&content_type)
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )
        .body(Body::from(bytes))
        .map_err(|e| internal_error(&ctx, format!("failed to build ui response: {}", e)))?;
    Ok(response)
}

pub async fn proxy_plugin_api(
    axum::Extension(_db): axum::Extension<Arc<sea_orm::DatabaseConnection>>,
    Path((plugin_id, path)): Path<(String, String)>,
    axum::Extension(ctx): axum::Extension<RequestContext>,
    req: Request,
) -> Result<Response, AppError> {
    let base_url = active_route_base_url(&plugin_id, &ctx).await?;
    let method = req.method().clone();
    let headers = req.headers().clone();
    let query = req.uri().query().unwrap_or("").to_string();
    let body = to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to read proxied request body: {}", e)))?;
    let target = if query.is_empty() {
        format!("{}/{}", base_url.trim_end_matches('/'), path)
    } else {
        format!("{}/{}?{}", base_url.trim_end_matches('/'), path, query)
    };
    let url = Url::parse(&target).map_err(|e| internal_error(&ctx, format!("invalid plugin api target url: {}", e)))?;
    let client = reqwest::Client::new();
    let mut builder = client.request(method, url);
    for (name, value) in &headers {
        builder = builder.header(name, value);
    }
    if let Some(user_info) = &ctx.user_info {
        builder = builder.header("X-Plugin-User-ID", user_info.user_id.as_ref());
        builder = builder.header("X-Plugin-User-Role", user_info.role_id.to_string());
        if let Some(username) = &user_info.username {
            builder = builder.header("X-Plugin-User-Name", username.as_ref());
        }
    }
    let response = builder
        .body(body)
        .send()
        .await
        .map_err(|e| internal_error(&ctx, format!("plugin api proxy failed: {}", e)))?;
    let mut resp_builder = Response::builder().status(response.status());
    for (name, value) in response.headers() {
        resp_builder = resp_builder.header(name, value);
    }
    let body = response
        .bytes()
        .await
        .map_err(|e| internal_error(&ctx, format!("failed to read plugin api response body: {}", e)))?;
    let resp = resp_builder
        .body(Body::from(body))
        .map_err(|e| internal_error(&ctx, format!("failed to build plugin api response: {}", e)))?;
    Ok(resp)
}

pub async fn proxy_plugin_ws_root(
    Path(plugin_id): Path<String>,
    ws: WebSocketUpgrade,
    OriginalUri(uri): OriginalUri,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Response, AppError> {
    proxy_plugin_ws_inner(plugin_id, String::new(), ws, uri, ctx).await
}

pub async fn proxy_plugin_ws(
    Path((plugin_id, path)): Path<(String, String)>,
    ws: WebSocketUpgrade,
    OriginalUri(uri): OriginalUri,
    axum::Extension(ctx): axum::Extension<RequestContext>,
) -> Result<Response, AppError> {
    proxy_plugin_ws_inner(plugin_id, path, ws, uri, ctx).await
}

async fn proxy_plugin_ws_inner(
    plugin_id: String,
    path: String,
    ws: WebSocketUpgrade,
    uri: axum::http::Uri,
    ctx: RequestContext,
) -> Result<Response, AppError> {
    let base_url = active_route_base_url(&plugin_id, &ctx).await?;
    let protocol = if base_url.starts_with("https://") || base_url.starts_with("wss://") {
        "wss://"
    } else {
        "ws://"
    };
    let authority = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("ws://")
        .trim_start_matches("wss://")
        .trim_end_matches('/');
    let query = uri.query().unwrap_or("");
    let target = if query.is_empty() {
        format!("{}{}/{}", protocol, authority, path)
    } else {
        format!("{}{}/{}?{}", protocol, authority, path, query)
    };
    let target_url = Url::parse(&target).map_err(|e| {
        AppError::new(
            ErrorCode::BadRequest,
            format!("invalid plugin websocket target: {}", e),
            ctx.request_id.to_owned(),
            ctx.client_ip.to_owned(),
        )
    })?;
    Ok(ws
        .on_upgrade(move |client_socket| async move {
            if let Err(e) = forward_websocket(client_socket, target_url).await {
                yh_console_log::yhlog("warn", &format!("plugin websocket proxy failed: {}", e));
            }
        })
        .into_response())
}

async fn forward_websocket(client_socket: WebSocket, target: Url) -> Result<(), String> {
    let (server_socket, _) = connect_async(target.as_str())
        .await
        .map_err(|e| format!("failed to connect upstream websocket: {}", e))?;
    let (mut client_sink, mut client_stream) = client_socket.split();
    let (mut server_sink, mut server_stream) = server_socket.split();

    let client_to_server = async {
        while let Some(Ok(msg)) = client_stream.next().await {
            let upstream = match msg {
                AxumWsMessage::Text(text) => TungsteniteMessage::Text(text.to_string().into()),
                AxumWsMessage::Binary(bin) => TungsteniteMessage::Binary(bin),
                AxumWsMessage::Ping(v) => TungsteniteMessage::Ping(v),
                AxumWsMessage::Pong(v) => TungsteniteMessage::Pong(v),
                AxumWsMessage::Close(frame) => {
                    let _ = server_sink.send(TungsteniteMessage::Close(frame.map(|f| {
                        tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: f.code.into(),
                            reason: f.reason.as_str().into(),
                        }
                    }))).await;
                    break;
                }
            };
            if server_sink.send(upstream).await.is_err() {
                break;
            }
        }
    };

    let server_to_client = async {
        while let Some(Ok(msg)) = server_stream.next().await {
            let downstream = match msg {
                TungsteniteMessage::Text(text) => AxumWsMessage::Text(text.to_string().into()),
                TungsteniteMessage::Binary(bin) => AxumWsMessage::Binary(bin),
                TungsteniteMessage::Ping(v) => AxumWsMessage::Ping(v),
                TungsteniteMessage::Pong(v) => AxumWsMessage::Pong(v),
                TungsteniteMessage::Close(frame) => {
                    let _ = client_sink.send(AxumWsMessage::Close(frame.map(|f| {
                        axum::extract::ws::CloseFrame {
                            code: f.code.into(),
                            reason: f.reason.as_str().into(),
                        }
                    }))).await;
                    break;
                }
                TungsteniteMessage::Frame(_) => continue,
            };
            if client_sink.send(downstream).await.is_err() {
                break;
            }
        }
    };

    tokio::select! {
        _ = client_to_server => {},
        _ = server_to_client => {},
    }

    Ok(())
}

pub fn create_public_router(_db: Arc<sea_orm::DatabaseConnection>) -> axum::Router {
    axum::Router::new()
        .route("/{plugin_id}/ui", axum::routing::get(serve_plugin_ui_root))
        .route("/{plugin_id}/ui/{*path}", axum::routing::get(serve_plugin_ui))
        .route("/{plugin_id}/api/{*path}", axum::routing::any(proxy_plugin_api))
        .route("/{plugin_id}/ws", axum::routing::get(proxy_plugin_ws_root))
        .route("/{plugin_id}/ws/{*path}", axum::routing::get(proxy_plugin_ws))
}
