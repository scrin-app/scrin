//! `scrin-service install <agent.exe> | uninstall | status | run <agent.exe>`.

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::path::PathBuf;
    use std::process::ExitCode;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("install") => match args.get(1) {
            Some(agent) => scrin_service::win::install(&PathBuf::from(agent)).map(|()| {
                println!("installed and started; Ctrl+Alt+Del generation enabled for services");
            }),
            None => Err("usage: scrin-service install <path to scrin agent>".into()),
        },
        Some("uninstall") => scrin_service::win::uninstall().map(|()| {
            println!("removed; Ctrl+Alt+Del policy restored");
        }),
        Some("status") => scrin_service::win::status().map(|s| println!("{s}")),
        Some("run") => scrin_service::win::run_dispatcher(),
        _ => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("scrin-service: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() {
    println!("{USAGE}\n(scrin-service is Windows-only)");
}

const USAGE: &str = "scrin-service — keeps an accepted scrin session working on the Windows \
lock screen and elevation prompts, and sends Ctrl+Alt+Del on request.

USAGE (as administrator):
  scrin-service install <agent.exe>   register, enable SoftwareSASGeneration, start
  scrin-service uninstall             stop, remove, restore the previous policy value
  scrin-service status                print the service state
  scrin-service run <agent.exe>       entry point used by the service manager";
