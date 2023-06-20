use actix_web::{web, App, Error, HttpResponse, HttpServer, HttpRequest, http::StatusCode};
use bollard::container::{AttachContainerOptions, AttachContainerResults, Config, LogOutput, ResizeContainerTtyOptions};
use bollard::Docker;
use bollard::image::CreateImageOptions;
use futures_util::{StreamExt, SinkExt, TryStreamExt};
use tokio::io::{AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use std::str::FromStr;
use tokio_tungstenite::accept_async;
use actix_web::web::Bytes;
use tokio::net::TcpStream;
use std::sync::Arc;
use tokio::sync::Mutex;

const IMAGE: &str = "alpine:3";

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .route("/ws/{id}", web::get().to(ws_route))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

async fn ws_route(req: HttpRequest, _: web::Payload) -> Result<HttpResponse, Error> {
    let id = req.match_info().get("id").unwrap_or("alpine");

    let docker = Docker::connect_with_socket_defaults().unwrap();

    docker
        .create_image(
            Some(CreateImageOptions {
                from_image: IMAGE,
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await
        .unwrap();

    let alpine_config = Config {
        image: Some(IMAGE),
        tty: Some(true),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        open_stdin: Some(true),
        ..Default::default()
    };

    let id = docker
        .create_container::<&str, &str>(None, alpine_config)
        .await
        .unwrap()
        .id;

    docker.start_container::<String>(&id, None).await.unwrap();

    let AttachContainerResults { mut output, mut input } = docker
        .attach_container(
            &id,
            Some(AttachContainerOptions::<String> {
                stdout: Some(true),
                stderr: Some(true),
                stdin: Some(true),
                stream: Some(true),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    let stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    let ws = Arc::new(Mutex::new(accept_async(stream).await.unwrap()));

    let ws_in = Arc::clone(&ws);
    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = ws_in.lock().await.next().await {
            match msg {
                Message::Text(text) => {
                    if let Err(e) = input.write_all(text.as_bytes()).await {
                        eprintln!("Error writing to input: {}", e);
                    }
                }
                Message::Binary(bin) => {
                    let size: Vec<&str> = std::str::from_utf8(&bin).unwrap().split(',').collect();
                    let height: u16 = u16::from_str(size[0]).unwrap();
                    let width: u16 = u16::from_str(size[1]).unwrap();
                    let _ = docker
                        .resize_container_tty(
                            &id,
                            ResizeContainerTtyOptions { width, height },
                        )
                        .await;
                }
                _ => {}
            }
        }
    });

    let ws_out = Arc::clone(&ws);
    actix_web::rt::spawn(async move {
        while let Some(Ok(data)) = output.next().await {
            match data {
                LogOutput::StdOut { message: bytes } | LogOutput::StdErr { message: bytes } => {
                    if let Err(e) = ws_out.lock().await.send(Message::Binary(Bytes::from(bytes).to_vec())).await {
                        eprintln!("Error sending WebSocket message: {}", e);
                    }
                },
                _ => {}
            }
        }
    });

    Ok(HttpResponse::build(StatusCode::OK).finish())
}
