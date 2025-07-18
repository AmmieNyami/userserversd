use std::collections::HashMap;
use std::env;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::exit;

use serde::de::DeserializeOwned;

use nix::unistd;

mod flag;
mod ipc;

use ipc::command::Command;
use ipc::response::{Response, ResponseKind, ResponseStatus};

fn get_home_directory() -> String {
    match env::var("HOME") {
        Ok(path) => path,
        Err(_) => {
            let uid = unistd::getuid();
            match unistd::User::from_uid(uid) {
                Ok(Some(user)) => {
                    let home = format!("/home/{}", user.name);
                    if !Path::new(&home).exists() {
                        eprintln!("ERROR: failed to get home directory path");
                        exit(1);
                    }
                    home
                }
                _ => {
                    eprintln!("ERROR: failed to get home directory path");
                    exit(1);
                }
            }
        }
    }
}

fn run_command(socket: &mut UnixStream, command: Command) -> Response {
    command.write_to_stream(socket).unwrap_or_else(|err| {
        eprintln!("ERROR: failed to send command to server: {err}");
        exit(1);
    });

    let response = Response::read_from_stream(socket)
        .unwrap_or_else(|err| {
            eprintln!("ERROR: failed to receive response from server: {err}");
            exit(1);
        })
        .unwrap_or_else(|| {
            eprintln!("ERROR: connection with server unexpectedly closed");
            exit(1);
        });

    if response.status != ResponseStatus::Ok {
        println!(
            "ERROR: command execution failed with the following status: {:?}",
            response.status
        );
        exit(1);
    }

    response
}

fn connect_to_socket() -> UnixStream {
    let socket_path = ipc::get_socket_path().unwrap_or_else(|err| {
        eprintln!("ERROR: failed to get socket path: {err}");
        exit(1);
    });

    match UnixStream::connect(socket_path) {
        Ok(sock) => sock,
        Err(err) => {
            println!("ERROR: failed to connect to socket: {err}");
            exit(1);
        }
    }
}

fn from_json<T: DeserializeOwned>(json: &String) -> T {
    serde_json::from_str(json.as_str()).unwrap_or_else(|err| {
        eprintln!("ERROR: invalid json was provided via the command line arguments: {err}");
        exit(1);
    })
}

fn cli() -> flag::Command {
    let mut root_command =
        flag::Command::new(None, "Add, remove, edit or query userserversd services.");

    let mut add_command = flag::Command::new(Some("add"), "Adds a new service.");

    let mut sync_subcommand = flag::Command::new(Some("sync"), "Adds a synchronous service with the specified name that runs the specified command. The command must be a JSON array, with each item being a command line argument.");
    sync_subcommand.add_positional_arg("service name", "The name of the service.");
    sync_subcommand.add_positional_arg("command", "The command that the service will run.");
    sync_subcommand.add_flag(
        "w",
        "working-directory",
        "Sets the working directory of the service to the provided argument.",
    );
    sync_subcommand.add_flag("e", "environment", "Overrides the environment variables of the service with the ones specified in the provided argument. The provided argument must be a JSON map.");
    sync_subcommand.add_flag(
        "g",
        "group",
        "Makes the service part of the group specified in the provided argument.",
    );

    let mut async_subcommand = flag::Command::new(Some("async"), "Adds an asynchronous service with the specified name that gets started with the specified start command and stopped with the specified stop command. The commands must be JSON arrays, with each item being a command line argument.");
    async_subcommand.add_positional_arg("service name", "The name of the service.");
    async_subcommand.add_positional_arg("start command", "The command that starts the service.");
    async_subcommand.add_positional_arg("stop command", "The command that stops the service.");
    async_subcommand.add_flag(
        "w",
        "working-directory",
        "Sets the working directory of the service to the provided argument.",
    );
    async_subcommand.add_flag("e", "environment", "Overrides the environment variables of the service with the ones specified in the provided argument. The provided argument must be a JSON map.");
    async_subcommand.add_flag(
        "g",
        "group",
        "Makes the service part of the group specified in the provided argument.",
    );

    add_command.add_subcommand(sync_subcommand);
    add_command.add_subcommand(async_subcommand);

    let mut remove_command = flag::Command::new(
        Some("remove"),
        "Removes the service with the specified name.",
    );
    remove_command.add_positional_arg("service name", "The name of the service.");

    let mut edit_command =
        flag::Command::new(Some("edit"), "Edits the service with the specified name.");

    let mut sync_subcommand = flag::Command::new(
        Some("sync"),
        "Edits the synchronous service with the specified name.",
    );
    sync_subcommand.add_positional_arg("service name", "The name of the service.");
    sync_subcommand.add_flag(
        "n",
        "name",
        "Changes the name of the service to the specified one.",
    );
    sync_subcommand.add_flag(
        "c",
        "command",
        "Changes the command of the service to the specified one.",
    );
    sync_subcommand.add_flag(
        "w",
        "working-directory",
        "Sets the working directory of the service to the provided argument.",
    );
    sync_subcommand.add_flag("e", "environment", "Overrides the environment variables of the service with the ones specified in the provided argument. The provided argument must be a JSON map.");
    sync_subcommand.add_flag(
        "g",
        "group",
        "Makes the service part of the group specified in the provided argument.",
    );

    let mut async_subcommand = flag::Command::new(
        Some("async"),
        "Edits the asynchronous service with the specified name.",
    );
    async_subcommand.add_positional_arg("service name", "The name of the service.");
    async_subcommand.add_flag(
        "n",
        "name",
        "Changes the name of the service to the specified one.",
    );
    async_subcommand.add_flag(
        "st",
        "start-command",
        "Changes the start command of the service to the specified one.",
    );
    async_subcommand.add_flag(
        "sp",
        "stop-command",
        "Changes the stop command of the service to the specified one.",
    );
    async_subcommand.add_flag(
        "w",
        "working-directory",
        "Sets the working directory of the service to the provided argument.",
    );
    async_subcommand.add_flag("e", "environment", "Overrides the environment variables of the service with the ones specified in the provided argument. The provided argument must be a JSON map.");
    async_subcommand.add_flag(
        "g",
        "group",
        "Makes the service part of the group specified in the provided argument.",
    );

    edit_command.add_subcommand(sync_subcommand);
    edit_command.add_subcommand(async_subcommand);

    let mut start_command =
        flag::Command::new(Some("start"), "Starts the service with the specified name.");
    start_command.add_positional_arg("service name", "The name of the service.");

    let mut stop_command =
        flag::Command::new(Some("stop"), "Stops the service with the specified name.");
    stop_command.add_positional_arg("service name", "The name of the service.");

    let mut restart_command = flag::Command::new(
        Some("restart"),
        "Restarts the service with the specified name.",
    );
    restart_command.add_positional_arg("service name", "The name of the service.");

    let mut status_command = flag::Command::new(
        Some("status"),
        "Displays the status of the service with the specified name.",
    );
    status_command.add_positional_arg("service name", "The name of the service.");

    let list_services_command = flag::Command::new(Some("list-services"), "List all services.");

    let help_command = flag::Command::new(Some("help"), "Prints this help.");

    root_command.add_subcommand(add_command);
    root_command.add_subcommand(remove_command);
    root_command.add_subcommand(edit_command);
    root_command.add_subcommand(start_command);
    root_command.add_subcommand(stop_command);
    root_command.add_subcommand(restart_command);
    root_command.add_subcommand(status_command);
    root_command.add_subcommand(list_services_command);
    root_command.add_subcommand(help_command);

    root_command
}

fn add_subcommand(subcommand: &flag::ParsedCommand) {
    let subcommand = subcommand.subcommand.as_ref().unwrap();

    let working_directory = subcommand
        .flags
        .get(&"working-directory".to_string())
        .map(|s| s.clone())
        .or(Some(get_home_directory()))
        .unwrap();
    let environment = subcommand
        .flags
        .get(&"environment".to_string())
        .map(|env| from_json(env))
        .or(Some(HashMap::new()))
        .unwrap();
    let group = subcommand
        .flags
        .get(&"group".to_string())
        .map(|s| s.clone());

    match subcommand.name.as_str() {
        "sync" => {
            let service_name = subcommand
                .positional_args
                .get(&"service name".to_string())
                .unwrap()
                .clone();

            let command = subcommand
                .positional_args
                .get(&"command".to_string())
                .unwrap();
            let command: Vec<String> = from_json(command);

            let mut socket = connect_to_socket();
            run_command(
                &mut socket,
                Command::AddSynchronousService {
                    name: service_name,
                    working_directory,
                    environment,
                    group,
                    command,
                },
            );
        }

        "async" => {
            let service_name = subcommand
                .positional_args
                .get(&"service name".to_string())
                .unwrap()
                .clone();

            let start_command = subcommand
                .positional_args
                .get(&"start command".to_string())
                .unwrap();
            let start_command: Vec<String> = from_json(start_command);

            let stop_command = subcommand
                .positional_args
                .get(&"stop command".to_string())
                .unwrap();
            let stop_command: Vec<String> = from_json(stop_command);

            let mut socket = connect_to_socket();
            run_command(
                &mut socket,
                Command::AddAsynchronousService {
                    name: service_name,
                    working_directory,
                    environment,
                    group,
                    start_command,
                    stop_command,
                },
            );
        }

        _ => unreachable!(),
    }
}

fn remove_subcommand(subcommand: &flag::ParsedCommand) {
    let service_name = subcommand
        .positional_args
        .get(&"service name".to_string())
        .unwrap()
        .clone();

    let mut socket = connect_to_socket();
    run_command(&mut socket, Command::RemoveService { name: service_name });
}

fn edit_subcommand(subcommand: &flag::ParsedCommand) {
    let subcommand = subcommand.subcommand.as_ref().unwrap();

    let service_name = subcommand
        .positional_args
        .get(&"service name".to_string())
        .unwrap()
        .clone();

    let mut socket = connect_to_socket();
    let get_service_response = run_command(
        &mut socket,
        Command::GetServiceStatus {
            name: service_name.clone(),
        },
    );
    let service = match get_service_response.kind {
        ResponseKind::ServiceStatus { service, .. } => service,
        _ => {
            eprintln!("ERROR: got unexpected response from servier");
            exit(1);
        }
    };

    let new_name = subcommand
        .flags
        .get(&"name".to_string())
        .map(|s| s.clone())
        .or(Some(service_name.clone()))
        .unwrap();
    let working_directory = subcommand
        .flags
        .get(&"working-directory".to_string())
        .map(|s| s.clone())
        .or(Some(service.working_directory))
        .unwrap();
    let environment = subcommand
        .flags
        .get(&"environment".to_string())
        .map(|env| from_json(env))
        .or(Some(service.environment))
        .unwrap();
    let group = subcommand
        .flags
        .get(&"group".to_string())
        .map(|s| s.clone())
        .or(service.group);

    let readd_command = match subcommand.name.as_str() {
        "sync" => {
            let old_command = if let ipc::ServiceKind::Synchronous { command } = service.kind {
                command
            } else {
                eprintln!("ERROR: service is not synchronous");
                exit(1);
            };

            let command = subcommand
                .flags
                .get(&"command".to_string())
                .map(|json| from_json(json))
                .or(Some(old_command))
                .unwrap();

            Command::AddSynchronousService {
                name: new_name,
                working_directory,
                environment,
                group,
                command,
            }
        }

        "async" => {
            let (old_start_command, old_stop_command) = if let ipc::ServiceKind::Asynchronous {
                start_command,
                stop_command,
            } = service.kind
            {
                (start_command, stop_command)
            } else {
                eprintln!("ERROR: service is not asynchronous");
                exit(1);
            };

            let start_command = subcommand
                .flags
                .get(&"start-command".to_string())
                .map(|json| from_json(json))
                .or(Some(old_start_command))
                .unwrap();
            let stop_command = subcommand
                .flags
                .get(&"stop-command".to_string())
                .map(|json| from_json(json))
                .or(Some(old_stop_command))
                .unwrap();

            Command::AddAsynchronousService {
                name: new_name,
                working_directory,
                environment,
                group,
                start_command,
                stop_command,
            }
        }

        _ => unreachable!(),
    };

    run_command(&mut socket, Command::RemoveService { name: service_name });
    run_command(&mut socket, readd_command);
}

fn start_subcommand(subcommand: &flag::ParsedCommand) {
    let service_name = subcommand
        .positional_args
        .get(&"service name".to_string())
        .unwrap()
        .clone();

    let mut socket = connect_to_socket();
    run_command(&mut socket, Command::StartService { name: service_name });
}

fn stop_subcommand(subcommand: &flag::ParsedCommand) {
    let service_name = subcommand
        .positional_args
        .get(&"service name".to_string())
        .unwrap()
        .clone();

    let mut socket = connect_to_socket();
    run_command(&mut socket, Command::StopService { name: service_name });
}

fn restart_subcommand(subcommand: &flag::ParsedCommand) {
    let service_name = subcommand
        .positional_args
        .get(&"service name".to_string())
        .unwrap()
        .clone();

    let mut socket = connect_to_socket();
    run_command(&mut socket, Command::RestartService { name: service_name });
}

fn status_subcommand(subcommand: &flag::ParsedCommand) {
    let service_name = subcommand
        .positional_args
        .get(&"service name".to_string())
        .unwrap()
        .clone();

    let mut socket = connect_to_socket();
    let response = run_command(
        &mut socket,
        Command::GetServiceStatus {
            name: service_name.clone(),
        },
    );

    if let ResponseKind::ServiceStatus {
        service,
        running,
        logs,
    } = response.kind
    {
        println!("Service status:");
        println!();
        println!("                 Name: {service_name}");
        println!("              Running: {running:?}");
        println!("    Working directory: {}", service.working_directory);
        println!("          Environment: {:?}", service.environment);
        if let Some(group) = service.group {
            println!("                Group: {group}")
        } else {
            println!("                Group: none")
        }
        match service.kind {
            ipc::ServiceKind::Synchronous { command } => {
                println!("              Command: {command:?}")
            }
            ipc::ServiceKind::Asynchronous {
                start_command,
                stop_command,
            } => {
                println!("        Start command: {start_command:?}");
                println!("         Stop command: {stop_command:?}");
            }
        }
        println!();
        println!("--- Beginning of Logs ---");
        println!("{logs}");
        println!("---    End of Logs    ---");
        println!();
    } else {
        eprintln!("ERROR: got unexpected response from server");
        exit(1);
    }
}

fn list_services_subcommand() {
    let mut socket = connect_to_socket();
    let response = run_command(&mut socket, Command::ListServices);

    let services = if let ResponseKind::ServiceList { services } = response.kind {
        services
    } else {
        eprintln!("ERROR: got unexpected response from server");
        exit(1);
    };

    // For truncating table values later.
    fn truncate_string(string: &String) -> String {
        let max_chars = 40;
        let truncated_length = max_chars.min(string.len());
        let mut truncated_string = string[..truncated_length].to_string();

        if truncated_length > 0 && truncated_length < string.len() {
            truncated_string.pop();
            truncated_string.push('|');
        }

        truncated_string
    }

    /*
     * Separate into groups.
     */
    let mut groups = HashMap::<String, HashMap<String, ipc::Service>>::new();
    for (service_name, service) in services {
        let group_name = match service.group {
            Some(ref group_name) => group_name.clone(),
            None => "none".to_string(),
        };

        let group = match groups.get_mut(&group_name) {
            Some(group) => group,
            None => {
                groups.insert(group_name.clone(), HashMap::new());
                groups.get_mut(&group_name).unwrap()
            }
        };

        group.insert(service_name, service);
    }

    /*
     * Get each property's displayed length.
     */
    let mut name_length = 4;
    let mut start_command_length = 13;
    let mut stop_command_length = 12;

    for (_, group) in &groups {
        for (service_name, service) in group {
            if truncate_string(service_name).len() > name_length {
                name_length = service_name.len();
            }

            match &service.kind {
                ipc::ServiceKind::Synchronous { command } => {
                    let formatted_command = truncate_string(&format!("{command:?}"));
                    if formatted_command.len() > start_command_length {
                        start_command_length = formatted_command.len();
                    }
                }

                ipc::ServiceKind::Asynchronous {
                    start_command,
                    stop_command,
                } => {
                    let formatted_start_command = truncate_string(&format!("{start_command:?}"));
                    let formatted_stop_command = truncate_string(&format!("{stop_command:?}"));

                    if formatted_start_command.len() > start_command_length {
                        start_command_length = formatted_start_command.len();
                    }

                    if formatted_stop_command.len() > stop_command_length {
                        stop_command_length = formatted_stop_command.len();
                    }
                }
            }
        }
    }

    /*
     * Display table.
     */
    for (group_name, group) in groups {
        println!("{group_name}:");
        println!(
            "    Name{}  Start Command{}  Stop Command{}",
            " ".repeat(name_length - "Name".len()),
            " ".repeat(start_command_length - "Start Command".len()),
            " ".repeat(stop_command_length - "Stop Command".len())
        );
        println!(
            "    {}",
            "-".repeat(name_length + start_command_length + stop_command_length + 4)
        );

        for (service_name, service) in group {
            print!(
                "    {service_name}{padding}  ",
                service_name = truncate_string(&service_name),
                padding = " ".repeat(name_length - service_name.len())
            );

            match service.kind {
                ipc::ServiceKind::Synchronous { command } => {
                    let formatted_command = truncate_string(&format!("{command:?}"));
                    println!(
                        "{formatted_command}{}  ",
                        " ".repeat(start_command_length - formatted_command.len())
                    );
                }

                ipc::ServiceKind::Asynchronous {
                    start_command,
                    stop_command,
                } => {
                    let formatted_start_command = truncate_string(&format!("{start_command:?}"));
                    print!(
                        "{formatted_start_command}{}  ",
                        " ".repeat(start_command_length - formatted_start_command.len())
                    );

                    let formatted_stop_command = truncate_string(&format!("{stop_command:?}"));
                    println!(
                        "{formatted_stop_command}{}",
                        " ".repeat(stop_command_length - formatted_stop_command.len())
                    );
                }
            }
        }
        println!();
    }
}

fn main() {
    let cli = cli();
    let parsed_cli = flag::parse(&cli).unwrap_or_else(|err| {
        eprintln!("{}", cli.generate_help());
        eprintln!("ERROR: {err}");
        exit(1);
    });

    let subcommand = parsed_cli.subcommand.unwrap();

    match subcommand.name.as_str() {
        "add" => add_subcommand(subcommand.as_ref()),
        "remove" => remove_subcommand(subcommand.as_ref()),
        "edit" => edit_subcommand(subcommand.as_ref()),
        "start" => start_subcommand(subcommand.as_ref()),
        "stop" => stop_subcommand(subcommand.as_ref()),
        "restart" => restart_subcommand(subcommand.as_ref()),
        "status" => status_subcommand(subcommand.as_ref()),
        "list-services" => list_services_subcommand(),

        "help" => {
            print!("{}", cli.generate_help());
            exit(0);
        }

        _ => unreachable!(),
    }

    println!("Command executed successfully!");
}
