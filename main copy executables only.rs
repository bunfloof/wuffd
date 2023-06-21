use actix::{Actor, Handler, Message, StreamHandler, AsyncContext};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use bollard::container::{LogOutput};
use bollard::Docker;
use bollard::exec::{CreateExecOptions, StartExecOptions};
use futures_util::StreamExt;
use tokio::runtime::Runtime;
use std::sync::{Arc, Mutex};

struct MyWebSocket {
    docker: Docker,
    container_name: String,
    log_output: Arc<Mutex<Vec<u8>>>,
}

impl Actor for MyWebSocket {
    type Context = ws::WebsocketContext<Self>;
}

#[derive(Message)]
#[rtype(result = "()")]
struct ExecuteCommand(String);

#[derive(Message)]
#[rtype(result = "()")]
struct CommandOutput(String);

impl Handler<ExecuteCommand> for MyWebSocket {
    type Result = ();

    fn handle(&mut self, msg: ExecuteCommand, ctx: &mut Self::Context) {
        let docker = Docker::connect_with_unix_defaults().unwrap();
        let container_name = self.container_name.clone();
        let options = CreateExecOptions {
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(true),
            cmd: Some(vec![msg.0]),
            ..Default::default()
        };
    
        let addr = ctx.address();
        std::thread::spawn(move || {
            let mut rt = Runtime::new().unwrap();
            let exec = rt.block_on(docker.create_exec(&container_name, options)).unwrap();
            let options = Some(StartExecOptions { detach: false, output_capacity: None });
            let output = rt.block_on(docker.start_exec(&exec.id, options)).unwrap();
            if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
                while let Some(Ok(log_output)) = rt.block_on(output.next()) {
                    match log_output {
                        LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                            let msg = String::from_utf8_lossy(&message).to_string();
                            addr.do_send(CommandOutput(msg));
                        }
                        _ => (),
                    }
                }
            }
        });
    }
}

impl Handler<CommandOutput> for MyWebSocket {
    type Result = ();

    fn handle(&mut self, msg: CommandOutput, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWebSocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Text(text)) => {
                ctx.address().do_send(ExecuteCommand(text.trim().to_string()));
            }
            _ => (),
        }
    }
}

async fn ws_index(
    r: HttpRequest,
    stream: web::Payload,
    info: web::Path<(String,)>,
) -> Result<HttpResponse, Error> {
    let docker = Docker::connect_with_local_defaults().unwrap();
    let res = ws::start(
        MyWebSocket {
            docker,
            container_name: info.0.clone(),
            log_output: Arc::new(Mutex::new(Vec::new())),
        },
        &r,
        stream,
    );
    res
}

#[actix_web::main]

async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new().route("/ws/{id}", web::get().to(ws_index))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
