use actix_web::{web, App, Error, HttpResponse, HttpServer};
use bollard::container::{Config, StartContainerOptions, RemoveContainerOptions};
use bollard::Docker;
use bollard::image::CreateImageOptions;
use futures_util::{StreamExt, TryStreamExt};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use std::str::FromStr;

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

async fn ws_route(req: web::HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let id = req.match_info().get("id").unwrap_or("alpine");

    let docker = Docker::connect_with_local_defaults().unwrap();

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

    let (response, mut ws) = actix_tungstenite::start_with_protocols(req, stream, &["docker"]).unwrap();

    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = ws.next().await {
            match msg {
                Message::Text(text) => {
                    let _ = input.write_all(text.as_bytes()).await;
                }
                Message::Binary(bin) => {
                    let size: Vec<&str> = std::str::from_utf8(&bin).unwrap().split(':').collect();
                    if size.len() == 2 {
                        if let (Ok(h), Ok(w)) = (u16::from_str(size[0]), u16::from_str(size[1])) {
                            let _ = docker.resize_container(&id, Some(ResizeContainerOptions { h, w })).await;
                        }
                    }
                }
                _ => {}
            }
        }
    });

    actix_web::rt::spawn(async move {
        let mut buffer = [0; 1024];
        while let Ok(message) = output.read(&mut buffer).await {
            let _ = ws.send(Message::Binary(buffer.to_vec())).await;
        }
    });

    Ok(response)
}
