use std::collections::{HashMap, VecDeque};
use std::env;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::exit;

use serde::de::DeserializeOwned;

use nix::unistd;

mod ipc;

use ipc::command::Command;
use ipc::response::{Response, ResponseKind, ResponseStatus};

const USAGE: &str = r#"USAGE: userserversctl <SUBCOMMAND>
    Add, remove, edit or query userserversd services.

SUBCOMMANDs:
    add <KIND> [OPTIONS]  Adds a new service.
        KINDs:
            sync <SERVICE NAME> <COMMAND>
                A synchronous service with name SERVICE NAME that runs
                the command COMMAND. COMMAND is a JSON array, where each
                item is a command line argument.

            asynchronous <SERVICE NAME> <START COMMAND> <STOP COMMAND>
                An asynchronous service with name SERVICE NAME that gets
                started with the command START COMMAND and stopped with
                the command STOP COMMAND. START COMMAND and STOP COMMAND
                are JSON arrays, where each item is a command line argument.

        OPTIONS:
            --working-directory <WORKING DIRECTORY>
                Sets the working directory of the service to WORKING DIRECTORY.

            --environment <ENVIRONMENT>
                Overrides the environment variables of the service with the
                ones specified in ENVIRONMENT. ENVIRONMENT is a JSON map.

            --group <GROUP>  Makes the service part of the group GROUP.

    remove <SERVICE NAME>  Removes the service with name SERVICE NAME.

    edit <KIND>  Edits a service.
        KINDs:
            sync <SERVICE NAME> [OPTIONS]
                OPTIONS:
                    --command <COMMAND>
                        Changes the command of the specified service to
                        COMMAND.

                    --working-directory <WORKING DIRECTORY>
                        Changes the working directory of the specified service
                        to WORKING DIRECTORY.

                    --environment <ENVIRONMENT>
                        Changes the environment of the specified service to
                        ENVIRONMENT.

                    --group <GROUP>
                        Changes the group of the specified service to GROUP.

            async <SERVICE NAME> [OPTIONS]
                OPTIONS:
                    --start-command <COMMAND>
                        Changes the start command of the specified service to
                        COMMAND.

                    --stop-command <COMMAND>
                        Changes the stop command of the specified service to
                        COMMAND.

                    --working-directory <WORKING DIRECTORY>
                        Changes the working directory of the specified service
                        to WORKING DIRECTORY.

                    --environment <ENVIRONMENT>
                        Changes the environment of the specified service to
                        ENVIRONMENT.

                    --group <GROUP>
                        Changes the group of the specified service to GROUP.

    start <SERVICE NAME>  Starts the service with name SERVICE NAME.

    stop <SERVICE NAME>  Stops the service with name SERVICE NAME.

    restart <SERVICE NAME>  Restarts the service with name SERVICE NAME.

    status <SERVICE NAME>
        Displays the status of the service with name SERVICE NAME.

    list-services  Lists all services.

    help  Prints this help."#;

fn usage(exit_code: i32) {
    if exit_code == 0 {
        println!("{USAGE}");
    } else {
        eprintln!("{USAGE}");
    }
    exit(exit_code);
}

fn truncate_string(string: &String) -> String {
    let max_chars = 85;
    let truncated_length = max_chars.min(string.len());
    let mut truncated_string = string[..truncated_length].to_string();

    if truncated_length > 0 && truncated_length < string.len() {
        truncated_string.pop();
        truncated_string.push('|');
    }

    truncated_string
}

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

fn from_json<T: DeserializeOwned>(json: String, provided_to: &str) -> T {
    serde_json::from_str(json.as_str()).unwrap_or_else(|err| {
        eprintln!("ERROR: invalid json was provided to {provided_to}: {err}");
        usage(1);
        loop {}
    })
}

fn parse_string_argument(args: &mut VecDeque<String>, name: &str, provided_to: &str) -> String {
    match args.pop_front() {
        Some(arg) => arg,
        None => {
            eprintln!("ERROR: no {name} was provided to {provided_to}");
            usage(1);
            loop {}
        }
    }
}

fn parse_flags_with_params(
    args: &mut VecDeque<String>,
    provided_to: &str,
) -> HashMap<String, String> {
    let mut flags = HashMap::new();

    while let Some(arg) = args.pop_front() {
        if !arg.starts_with("--") {
            eprintln!("ERROR: unknown flag provided to {provided_to}: {arg}");
            usage(1);
        }

        match args.pop_front() {
            Some(param) => flags.insert(arg, param),
            None => {
                eprintln!("ERROR: no parameter was provided the specified flag");
                usage(1);
                loop {}
            }
        };
    }

    flags
}

fn parse_workdir_env_group(
    args: &mut VecDeque<String>,
    provided_to: &str,
) -> (String, HashMap<String, String>, Option<String>) {
    let mut flags = parse_flags_with_params(args, provided_to);

    let working_directory = match flags.remove(&"--working-directory".to_string()) {
        Some(working_directory) => working_directory,
        None => get_home_directory(),
    };

    let environment = match flags.remove(&"--environment".to_string()) {
        Some(environment) => from_json(environment, "the `--environment` flag"),
        None => HashMap::new(),
    };

    let group = flags.remove(&"--group".to_string());

    for (flag, _) in flags {
        eprintln!("ERROR: unknown flag provided to {provided_to}: {flag}");
        usage(1);
    }

    (working_directory, environment, group)
}

fn main() {
    let mut args: VecDeque<String> = env::args().collect();
    args.pop_front().unwrap();

    let subcommand = args.pop_front();
    match subcommand {
        Some(ref s) if s.as_str() == "add" => {
            let service_kind = args.pop_front();
            match service_kind {
                Some(ref s) if s.as_str() == "sync" => {
                    let service_name =
                        parse_string_argument(&mut args, "service name", "the `add` subcommand");
                    let command =
                        parse_string_argument(&mut args, "command", "the `add` subcommand");
                    let command: Vec<String> = from_json(command, "the `add` subcommand");

                    let (working_directory, environment, group) =
                        parse_workdir_env_group(&mut args, "the `add` subcommand");

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

                Some(ref s) if s.as_str() == "async" => {
                    let service_name =
                        parse_string_argument(&mut args, "service name", "the `add` subcommand");

                    let start_command =
                        parse_string_argument(&mut args, "start command", "the `add` subcommand");
                    let start_command: Vec<String> =
                        from_json(start_command, "the `add` subcommand");

                    let stop_command =
                        parse_string_argument(&mut args, "stop command", "the `add` subcommand");
                    let stop_command: Vec<String> = from_json(stop_command, "the `add` subcommand");

                    let (working_directory, environment, group) =
                        parse_workdir_env_group(&mut args, "the `add` subcommand");

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

                Some(service_kind) => {
                    eprintln!("ERROR: unknown service kind: {service_kind}");
                    usage(1);
                }

                None => {
                    eprintln!("ERROR: no service kind was provided to the `add` subcommand");
                    usage(1);
                }
            }
        }

        Some(ref s) if s.as_str() == "remove" => {
            let service_name =
                parse_string_argument(&mut args, "service name", "the `remove` subcommand");

            let mut socket = connect_to_socket();
            run_command(&mut socket, Command::RemoveService { name: service_name });
        }

        Some(ref s) if s.as_str() == "edit" => {
            let service_kind = args.pop_front();
            let service_kind = match service_kind.as_deref() {
                Some("sync") | Some("async") => service_kind.unwrap(),

                Some(service_kind) => {
                    eprintln!("ERROR: unknown service kind: {service_kind}");
                    usage(1);
                    loop {}
                }

                None => {
                    eprintln!("ERROR: no service kind was provided to the `edit` subcommand");
                    usage(1);
                    loop {}
                }
            };

            let service_name =
                parse_string_argument(&mut args, "service name", "the `remove` subcommand");

            let mut flags = parse_flags_with_params(&mut args, "the `remove` subcommand");

            let new_name = flags.remove(&"--name".to_string());
            let new_working_directory = flags.remove(&"--working-directory".to_string());
            let new_environment: Option<HashMap<String, String>> = flags
                .remove(&"--environment".to_string())
                .map(|json| from_json(json, "the `--environment` flag"));
            let new_group = flags.remove(&"--group".to_string());

            enum ServiceCommand {
                Synchronous(Option<Vec<String>>),
                Asynchronous(Option<Vec<String>>, Option<Vec<String>>),
            }

            let new_service_command = match service_kind.as_str() {
                "sync" => {
                    let command: Option<Vec<String>> = flags
                        .remove(&"--command".to_string())
                        .map(|json| from_json(json, "the `--environment` flag"));

                    for (flag, _) in flags {
                        eprintln!("ERROR: unknown flag provided to the `edit` subcommand: {flag}");
                        usage(1);
                    }

                    ServiceCommand::Synchronous(command)
                }

                "async" => {
                    let start_command: Option<Vec<String>> = flags
                        .remove(&"--start-command".to_string())
                        .map(|json| from_json(json, "the `--environment` flag"));

                    let stop_command: Option<Vec<String>> = flags
                        .remove(&"--stop-command".to_string())
                        .map(|json| from_json(json, "the `--environment` flag"));

                    for (flag, _) in flags {
                        eprintln!("ERROR: unknown flag provided to the `edit` subcommand: {flag}");
                        usage(1);
                    }

                    ServiceCommand::Asynchronous(start_command, stop_command)
                }

                _ => unreachable!(),
            };

            // Flags parsed. Onto the actual editing.

            let mut socket = connect_to_socket();

            let get_service_status_response = run_command(
                &mut socket,
                Command::GetServiceStatus {
                    name: service_name.clone(),
                },
            );

            let old_service = match get_service_status_response.kind {
                ResponseKind::ServiceStatus { service, .. } => service,
                _ => {
                    eprintln!("ERROR: got unexpected response from server");
                    exit(1);
                }
            };

            let new_name = new_name.or(Some(service_name.clone())).unwrap();
            let new_working_directory = new_working_directory
                .or(Some(old_service.working_directory))
                .unwrap();
            let new_environment = new_environment.or(Some(old_service.environment)).unwrap();
            let new_group = new_group.or(old_service.group);

            let new_service_command = match (new_service_command, old_service.kind) {
                (
                    ServiceCommand::Synchronous(command),
                    ipc::ServiceKind::Synchronous {
                        command: old_command,
                    },
                ) => {
                    let command = match command {
                        Some(command) => command,
                        None => old_command,
                    };

                    Command::AddSynchronousService {
                        name: new_name,
                        working_directory: new_working_directory,
                        environment: new_environment,
                        group: new_group,
                        command,
                    }
                }

                (
                    ServiceCommand::Asynchronous(start_command, stop_command),
                    ipc::ServiceKind::Asynchronous {
                        start_command: old_start_command,
                        stop_command: old_stop_command,
                    },
                ) => {
                    let start_command = match start_command {
                        Some(start_command) => start_command,
                        None => old_start_command,
                    };

                    let stop_command = match stop_command {
                        Some(stop_command) => stop_command,
                        None => old_stop_command,
                    };

                    Command::AddAsynchronousService {
                        name: new_name,
                        working_directory: new_working_directory,
                        environment: new_environment,
                        group: new_group,
                        start_command,
                        stop_command,
                    }
                }

                (ServiceCommand::Synchronous(_), ipc::ServiceKind::Asynchronous { .. }) => {
                    eprintln!("ERROR: the specified service is not synchronous");
                    exit(1);
                }

                (ServiceCommand::Asynchronous { .. }, ipc::ServiceKind::Synchronous { .. }) => {
                    eprintln!("ERROR: the specified service is not asynchronous");
                    exit(1);
                }
            };

            run_command(&mut socket, Command::RemoveService { name: service_name });
            run_command(&mut socket, new_service_command);
        }

        Some(ref s) if s.as_str() == "start" => {
            let service_name =
                parse_string_argument(&mut args, "service name", "the `start` subcommand");

            let mut socket = connect_to_socket();
            run_command(&mut socket, Command::StartService { name: service_name });
        }

        Some(ref s) if s.as_str() == "stop" => {
            let service_name =
                parse_string_argument(&mut args, "service name", "the `stop` subcommand");

            let mut socket = connect_to_socket();
            run_command(&mut socket, Command::StopService { name: service_name });
        }

        Some(ref s) if s.as_str() == "restart" => {
            let service_name =
                parse_string_argument(&mut args, "service name", "the `restart` subcommand");

            let mut socket = connect_to_socket();
            run_command(&mut socket, Command::RestartService { name: service_name });
        }

        Some(ref s) if s.as_str() == "status" => {
            let service_name =
                parse_string_argument(&mut args, "service name", "the `status` subcommand");

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

        Some(ref s) if s.as_str() == "list-services" => {
            let mut socket = connect_to_socket();
            let response = run_command(&mut socket, Command::ListServices);

            if let ResponseKind::ServiceList { services } = response.kind {
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
                                    " ".repeat(
                                        start_command_length - formatted_start_command.len()
                                    )
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
            } else {
                eprintln!("ERROR: got unexpected response from server");
                exit(1);
            }
        }

        Some(ref s) if s.as_str() == "help" => usage(0),

        Some(subcommand) => {
            eprintln!("ERROR: unknown subcommand: {subcommand}");
            usage(1);
        }

        None => {
            eprintln!("ERROR: no subcommand was provided");
            usage(1);
        }
    }

    println!("Command executed successfully!");
}
