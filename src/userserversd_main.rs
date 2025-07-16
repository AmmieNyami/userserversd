use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::exit;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use signal_hook::consts as sigconsts;
use signal_hook::iterator::Signals;

mod ipc;
mod service;
mod service_manager;

use ipc::command::Command;
use ipc::response::{Response, ResponseKind, ResponseStatus};

use service_manager::ServiceManager;

fn handle_client(stream: &mut UnixStream, service_manager: Arc<Mutex<ServiceManager>>) {
    loop {
        let command = match Command::read_from_stream(stream) {
            Ok(Some(command)) => command,
            Ok(None) => break,
            Err(_) => continue,
        };

        println!("Received command: {:?}", command);

        let mut service_manager = service_manager.lock().unwrap();
        let response = match command {
            Command::AddSynchronousService {
                name,
                working_directory,
                environment,
                group,
                command,
            } => service_manager.add_synchronous(
                name,
                working_directory,
                environment,
                group,
                command,
            ),

            Command::AddAsynchronousService {
                name,
                working_directory,
                environment,
                group,
                start_command,
                stop_command,
            } => service_manager.add_asynchronous(
                name,
                working_directory,
                environment,
                group,
                start_command,
                stop_command,
            ),

            Command::RemoveService { name } => service_manager.remove(name),

            Command::StartService { name } => service_manager.start(name),
            Command::StopService { name } => service_manager.stop(name),
            Command::RestartService { name } => service_manager.restart(name),

            Command::GetServiceStatus { name } => service_manager.get_status(name),
            Command::ListServices => service_manager.list_services(),
        };

        let response = match response {
            Ok(kind) => Response {
                status: ResponseStatus::Ok,
                kind,
            },
            Err(status) => {
                println!(
                    "Command execution failed with the following status: {:?}",
                    status
                );

                Response {
                    status,
                    kind: ResponseKind::None,
                }
            }
        };

        response.write_to_stream(stream).unwrap_or_else(|err| {
            println!("Failed to send response to client: {err}");
        });
    }
}

fn server(
    socket_path: String,
    service_manager: Arc<Mutex<ServiceManager>>,
    exit_code_tx: Arc<Mutex<mpsc::Sender<i32>>>,
) {
    let listener = UnixListener::bind(&socket_path).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to bind socket: {err}");
        exit_code_tx.lock().unwrap().send(1).unwrap();
        loop {}
    });

    println!("Listening for commands on socket `{socket_path}`");

    for stream in listener.incoming() {
        let mut stream = stream.unwrap_or_else(|err| {
            eprintln!("ERROR: failed to accept connection: {err}");
            exit_code_tx.lock().unwrap().send(1).unwrap();
            loop {}
        });

        let handle_client_services = service_manager.clone();
        thread::spawn(move || handle_client(&mut stream, handle_client_services));
    }
}

fn main() {
    let service_manager = Arc::new(Mutex::new(ServiceManager::new()));

    let (exit_code_tx, exit_code_rx) = mpsc::channel();
    let exit_code_tx = Arc::new(Mutex::new(exit_code_tx));

    /*
     * Setup server thread.
     */

    let socket_path = ipc::get_socket_path().unwrap_or_else(|err| {
        eprintln!("ERROR: failed to get socket path: {err}");
        exit(1);
    });

    let server_service_manager = service_manager.clone();
    let server_exit_code_tx = exit_code_tx.clone();
    let server_socket_path = socket_path.clone();
    thread::spawn(move || {
        server(
            server_socket_path,
            server_service_manager,
            server_exit_code_tx,
        )
    });

    /*
     * Setup signal handler thread.
     */

    let mut signals =
        Signals::new(&[sigconsts::SIGINT, sigconsts::SIGTERM]).unwrap_or_else(|err| {
            eprintln!("ERROR: failed to set up signal handlers: {err}");
            exit(1);
        });

    let signal_handler_exit_code_tx = exit_code_tx.clone();
    thread::spawn(move || {
        for _ in signals.forever() {
            signal_handler_exit_code_tx.lock().unwrap().send(0).unwrap();
        }
    });

    /*
     * Listen to receiver channel.
     */

    loop {
        let exit_code = exit_code_rx.recv().unwrap();

        service_manager.lock().unwrap().stop_all();
        if exit_code == 0 {
            fs::remove_file(socket_path).unwrap_or_else(|err| {
                eprintln!("ERROR: failed to remove socket file: {err}");
                exit(1);
            });
        }

        exit(exit_code);
    }
}
