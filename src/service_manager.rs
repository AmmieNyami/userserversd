use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io;
use std::io::Write;
use std::path::Path;

use nix::unistd;

use super::ipc;
use super::ipc::response::{ResponseKind, ResponseStatus};

use super::service::{Service, ServiceKind};

fn service_to_ipc_service(service: &Service) -> ipc::Service {
    ipc::Service {
        working_directory: service.working_directory.clone(),
        environment: service.environment.clone(),
        group: service.group.clone(),
        kind: match &service.kind {
            ServiceKind::Synchronous { command } => ipc::ServiceKind::Synchronous {
                command: command.clone(),
            },

            ServiceKind::Asynchronous {
                start_command,
                stop_command,
            } => ipc::ServiceKind::Asynchronous {
                start_command: start_command.clone(),
                stop_command: stop_command.clone(),
            },
        },
    }
}

fn get_config_file_path() -> Option<String> {
    if let Ok(config_dir) = env::var("XDG_CONFIG_HOME") {
        return Some(format!("{config_dir}/userserversd_services.json"));
    }

    let home = match env::var("HOME") {
        Ok(path) => path,
        Err(_) => {
            let uid = unistd::getuid();
            match unistd::User::from_uid(uid) {
                Ok(Some(user)) => {
                    let home = format!("/home/{}", user.name);
                    if !Path::new(&home).exists() {
                        return None;
                    }
                    home
                }
                _ => return None,
            }
        }
    };

    let config_file = if Path::new(&format!("{home}/.userserversd_services.json")).exists()
        || !Path::new(&format!("{home}/.config")).exists()
    {
        format!("{home}/.userserversd_services.json")
    } else {
        format!("{home}/.config/userserversd_services.json")
    };

    Some(config_file)
}

pub struct ServiceManager {
    services: HashMap<String, Service>,
}

impl ServiceManager {
    pub fn new() -> Self {
        let mut selff = Self {
            services: HashMap::<String, Service>::new(),
        };

        let config_file_path = match get_config_file_path() {
            Some(path) => path,
            None => {
                println!(
                    "Failed to get path for configuration file. Service list will NOT be loaded!"
                );
                return selff;
            }
        };

        let config_file_contents = match fs::read_to_string(&config_file_path) {
            Ok(contents) => contents,
            Err(err) => {
                if err.kind() != io::ErrorKind::NotFound {
                    println!(
                        "Failed to read configuration file for the following reason: {err}. Service list will NOT be loaded!"
                    );
                }
                return selff;
            }
        };

        match serde_json::from_str(&config_file_contents) {
            Ok(services) => selff.services = services,
            Err(err) => println!(
                "Failed to deserialize configuration file for the following reason: {err}. Service list will NOT be loaded!"
            ),
        }

        println!("Starting services...");

        for (service_name, service) in &mut selff.services {
            println!("Starting service `{service_name}`");
            if let Err(err) = service.start() {
                println!("Failed to start service `{service_name}`: {err}");
            }
        }

        return selff;
    }

    fn flush(&self) {
        let config_file_path = match get_config_file_path() {
            Some(path) => path,
            None => {
                println!(
                    "Failed to get path for configuration file. Service list will NOT be saved!"
                );
                return;
            }
        };

        let mut config_file = match File::create(config_file_path) {
            Ok(file) => file,
            Err(err) => {
                println!("Failed to create configuration file: {err}");
                return;
            }
        };

        match serde_json::to_string(&self.services) {
            Ok(string) => {
                if let Err(err) = write!(config_file, "{string}") {
                    println!(
                        "Failed to write configuration file for the following reason: {err}. Service list will NOT be saved!"
                    );
                    return;
                }
            }
            Err(err) => {
                println!(
                    "Failed to serialize configuration file for the following reason: {err}. Service list will NOT be saved!"
                );
                return;
            }
        }
    }

    fn get_service_mut(&mut self, name: &String) -> Result<&mut Service, ResponseStatus> {
        match self.services.get_mut(name) {
            Some(service) => Ok(service),
            None => Err(ResponseStatus::ServiceDoesNotExist),
        }
    }

    fn get_service(&self, name: &String) -> Result<&Service, ResponseStatus> {
        match self.services.get(name) {
            Some(service) => Ok(service),
            None => Err(ResponseStatus::ServiceDoesNotExist),
        }
    }

    pub fn add_synchronous(
        &mut self,

        name: String,
        working_directory: String,
        environment: HashMap<String, String>,
        group: Option<String>,

        command: Vec<String>,
    ) -> Result<ResponseKind, ResponseStatus> {
        println!("Adding service `{name}`");

        if self.services.contains_key(&name) {
            return Err(ResponseStatus::ServiceAlreadyExists);
        }

        self.services.insert(
            name.clone(),
            Service::new(
                working_directory,
                environment,
                group,
                ServiceKind::Synchronous { command },
            ),
        );

        println!("Starting service `{name}`");
        if let Err(err) = self.services.get_mut(&name).unwrap().start() {
            println!("Failed to start service `{name}`: {err}");
        }

        self.flush();

        Ok(ResponseKind::None)
    }

    pub fn add_asynchronous(
        &mut self,

        name: String,
        working_directory: String,
        environment: HashMap<String, String>,
        group: Option<String>,

        start_command: Vec<String>,
        stop_command: Vec<String>,
    ) -> Result<ResponseKind, ResponseStatus> {
        println!("Adding service `{name}`");

        if self.services.contains_key(&name) {
            return Err(ResponseStatus::ServiceAlreadyExists);
        }

        self.services.insert(
            name.clone(),
            Service::new(
                working_directory,
                environment,
                group,
                ServiceKind::Asynchronous {
                    start_command,
                    stop_command,
                },
            ),
        );

        println!("Starting service `{name}`");
        if let Err(err) = self.services.get_mut(&name).unwrap().start() {
            println!("Failed to start service `{name}`: {err}");
        }

        self.flush();

        Ok(ResponseKind::None)
    }

    pub fn remove(&mut self, name: String) -> Result<ResponseKind, ResponseStatus> {
        println!("Removing service `{name}`");

        let service = self.get_service_mut(&name)?;

        println!("Stopping service `{name}`");
        if service.is_running()
            && let Err(err) = service.stop()
        {
            println!("Failed to stop service `{name}`: {err}");
        }

        self.services.remove(&name);
        println!("Service removed");

        self.flush();

        Ok(ResponseKind::None)
    }

    pub fn start(&mut self, name: String) -> Result<ResponseKind, ResponseStatus> {
        let service = self.get_service_mut(&name)?;

        println!("Starting service `{name}`");
        if let Err(err) = service.start() {
            println!("Failed to start service `{name}`: {err}");
        }

        Ok(ResponseKind::None)
    }

    pub fn stop(&mut self, name: String) -> Result<ResponseKind, ResponseStatus> {
        let service = self.get_service_mut(&name)?;

        println!("Stopping service `{name}`");
        if let Err(err) = service.stop() {
            println!("Failed to stop service `{name}`: {err}");
        }

        Ok(ResponseKind::None)
    }

    pub fn restart(&mut self, name: String) -> Result<ResponseKind, ResponseStatus> {
        let service = self.get_service_mut(&name)?;

        println!("Restarting service `{name}`");
        if let Err(err) = service.restart() {
            println!("Failed to restart service `{name}`: {err}");
        }

        Ok(ResponseKind::None)
    }

    pub fn stop_all(&mut self) {
        println!("Stopping services...");

        for (service_name, service) in &mut self.services {
            println!("Stopping service `{service_name}`");
            if service.is_running()
                && let Err(err) = service.stop()
            {
                println!("Failed to stop service `{service_name}`: {err}");
            }
        }
    }

    pub fn get_status(&self, name: String) -> Result<ResponseKind, ResponseStatus> {
        let service = self.get_service(&name)?;

        Ok(ResponseKind::ServiceStatus {
            service: service_to_ipc_service(&service),
            running: service.is_running(),
            logs: service.get_logs(),
        })
    }

    pub fn list_services(&self) -> Result<ResponseKind, ResponseStatus> {
        let mut services = HashMap::<String, ipc::Service>::new();
        for (k, v) in &self.services {
            services.insert(k.clone(), service_to_ipc_service(v));
        }

        Ok(ResponseKind::ServiceList { services })
    }
}
