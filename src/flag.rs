use std::collections::HashMap;
use std::env;
use std::iter::Peekable;

#[derive(Clone)]
struct Flag {
    name: (String, String),
    help: String,
}

#[derive(Clone)]
pub struct Command {
    name: Option<String>,
    help: String,
    flags: Vec<Flag>,
    positional_args: Vec<(String, String)>,
    subcommands: Vec<Command>,
}

impl Command {
    pub fn new(name: Option<&str>, help: &str) -> Self {
        Self {
            name: name.map(|s| s.to_string()),
            help: help.to_string(),
            flags: Vec::new(),
            positional_args: Vec::new(),
            subcommands: Vec::new(),
        }
    }

    pub fn add_flag(&mut self, short_name: &str, long_name: &str, help: &str) {
        self.flags.push(Flag {
            name: (short_name.to_string(), long_name.to_string()),
            help: help.to_string(),
        })
    }

    pub fn add_subcommand(&mut self, subcommand: Command) {
        self.subcommands.push(subcommand);
    }

    pub fn add_positional_arg(&mut self, name: &str, help: &str) {
        self.positional_args
            .push((name.to_string(), help.to_string()));
    }

    fn generate_help_impl(&self, indentation: usize) -> String {
        let command_name = match &self.name {
            Some(name) => name,
            None => &env::args().next().unwrap(),
        };

        let mut output = String::new();
        let indent_str = " ".repeat(indentation);

        // Usage line
        output.push_str(&indent_str);
        if self.name.is_none() {
            output.push_str(&format!("USAGE: {command_name}"));
        } else {
            output.push_str(&format!("{command_name}"));
        }

        // Add positional arguments
        for (arg, _) in &self.positional_args {
            output.push_str(&format!(" <{}>", arg.to_uppercase()));
        }

        // Add optional parts
        if !self.flags.is_empty() {
            output.push_str(" [OPTIONS]");
        }
        if !self.subcommands.is_empty() {
            output.push_str(" <SUBCOMMAND>");
        }

        // Description
        output.push_str(&format!("\n    {indent_str}{}\n", self.help));

        // Positional arguments
        if !self.positional_args.is_empty() {
            for (arg_name, arg_help) in &self.positional_args {
                output.push_str(&format!("\n{indent_str}{}:\n", arg_name.to_uppercase()));
                output.push_str(&format!("{indent_str}    {arg_help}\n"));
            }
        }

        // Flags
        if !self.flags.is_empty() {
            output.push_str(&format!("\n{indent_str}OPTIONS:\n"));
            for (i, flag) in self.flags.iter().enumerate() {
                if i != 0 {
                    output.push('\n');
                }

                output.push_str(&format!(
                    "{indent_str}    -{}, --{}  <ARGUMENT>\n",
                    flag.name.0, flag.name.1
                ));
                output.push_str(&format!("{indent_str}        {}\n", flag.help));
            }
        }

        // Subcommands
        if !self.subcommands.is_empty() {
            output.push_str(&format!("\n{indent_str}SUBCOMMANDS:\n"));
            for (i, subcommand) in self.subcommands.iter().enumerate() {
                if i != 0 {
                    output.push('\n');
                    output.push('\n');
                }
                output.push_str(&subcommand.generate_help_impl(indentation + 4));
            }
        }

        output
    }

    pub fn generate_help(&self) -> String {
        self.generate_help_impl(0)
    }
}

struct Parser {
    argv: Peekable<env::Args>,
    program_name: String,
}

impl Parser {
    fn new() -> Self {
        let mut parser = Self {
            argv: env::args().peekable(),
            program_name: String::new(),
        };
        parser.program_name = parser.argv.next().unwrap();
        parser
    }

    fn parse(&mut self, command: &Command) -> Result<ParsedCommand, String> {
        let mut parsed_command = ParsedCommand {
            name: match &command.name {
                Some(name) => name.clone(),
                None => self.program_name.clone(),
            },
            flags: HashMap::new(),
            positional_args: HashMap::new(),
            subcommand: None,
        };

        for (arg_name, _) in &command.positional_args {
            let arg = match self.argv.next() {
                Some(arg) => arg,
                None => {
                    return match &command.name {
                        Some(name) => Err(format!(
                            "no {arg_name} was provided to the {name} subcommand"
                        )),
                        None => Err(format!("no {arg_name} was provided")),
                    }
                }
            };

            parsed_command.positional_args.insert(arg_name.clone(), arg);
        }

        if !command.flags.is_empty() {
            while let Some(arg) = self.argv.peek() {
                if !arg.starts_with("-") {
                    break;
                }
                let arg = self.argv.next().unwrap();

                let mut flag_known = false;
                for flag in &command.flags {
                    if format!("-{}", flag.name.0) == arg || format!("--{}", flag.name.1) == arg {
                        let flag_argument = match self.argv.next() {
                            Some(flag_argument) => flag_argument,
                            None => {
                                return Err(format!(
                                    "the flag {arg} is missing a positional argument"
                                ));
                            }
                        };
                        parsed_command
                            .flags
                            .insert(flag.name.1.clone(), flag_argument);

                        flag_known = true;
                        break;
                    }
                }

                if !flag_known {
                    return Err(format!("unknown flag: {arg}"));
                }
            }
        }

        if !command.subcommands.is_empty() {
            let arg = match self.argv.next() {
                Some(arg) => arg,
                None => {
                    return match &command.name {
                        Some(name) => Err(format!(
                            "no subcommand was provided to the {name} subcommand"
                        )),
                        None => Err("no subcommand was provided".to_string()),
                    }
                }
            };

            for subcommand in &command.subcommands {
                let subcommand_name = match &subcommand.name {
                    Some(name) => name,
                    None => return Err("all subcommands must have names".to_string()),
                };

                if *subcommand_name == arg {
                    parsed_command.subcommand = Some(Box::new(self.parse(subcommand)?));
                    break;
                }
            }

            if parsed_command.subcommand.is_none() {
                return match &command.name {
                    Some(name) => Err(format!(
                        "an unknown subcommand was provided to the {name} subcommand"
                    )),
                    None => Err(format!("unknown subcommand: {arg}")),
                };
            }
        }

        Ok(parsed_command)
    }
}

pub fn parse(command: &Command) -> Result<ParsedCommand, String> {
    let mut parser = Parser::new();
    parser.parse(command)
}

#[derive(Clone)]
pub struct ParsedCommand {
    pub name: String,
    pub flags: HashMap<String, String>,
    pub positional_args: HashMap<String, String>,
    pub subcommand: Option<Box<ParsedCommand>>,
}
