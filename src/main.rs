#[macro_use]
extern crate log;

use console::Term;
use directories::BaseDirs;
use env_logger::Env;
use std::{
    env,
    error::Error,
    io::{stdout, Cursor, Write},
    net::TcpStream,
    path::Path,
    process::Stdio,
};
use tokio::{fs, process::Command};
use yansi::Paint;

const PYTHON37ZIP: &str = "https://github.com/RLBot/RLBotGUI/raw/master/alternative-install/python-3.7.9-custom-amd64.zip";

fn is_online() -> bool {
    match TcpStream::connect("google.com:80") {
        Ok(_) => true,
        Err(_) => false,
    }
}

async fn run_bat(s: &str) -> Result<(), Box<dyn Error>> {
    let tmp_file = env::temp_dir().join("rlbotguilaunchertemp.bat");
    fs::write(&tmp_file, s).await?;
    Command::new("cmd")
        .args(["/C", tmp_file.to_str().unwrap()])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;

    Ok(())
}

async fn realmain() -> Result<(), Box<dyn Error>> {
    let rlbot_banner = include_str!("../assets/rlbot-banner.txt");
    println!("{}\n", rlbot_banner.green());
    info!("Checking for internet connection...");

    let is_online = is_online();

    info!("Online status: {is_online}");

    // Check for self update
    // TODO

    let base_dirs = BaseDirs::new().ok_or("Couldn't get BaseDirs")?;

    // Check for RLBotGUIX path
    let rlbotgui_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX");
    if !rlbotgui_dir.exists() {
        fs::create_dir_all(rlbotgui_dir).await?;
    }

    // Check for python install
    let python_install_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/Python37");
    if !python_install_dir.exists() {
        info!("Python not found, installing...");
        if !is_online {
            Err("RLBot needs python to function and can't download it since you're offline. Please connect to the internet and try again.")?
        }
        let zip = reqwest::get(PYTHON37ZIP).await?.bytes().await?;
        zip_extract::extract(Cursor::new(zip), &python_install_dir, true)?;
        info!("Python installed")
    }

    let rlbot_python = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/Python37/python.exe");

    let venv_activate_path = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/venv");
    let venv_activate_bat = Path::join(Path::new(&venv_activate_path), "Scripts/activate.bat");
    let venv_exists = venv_activate_bat.exists();
    if !venv_exists {
        info!("No RLBot virtual python environment found, creating...");
        Command::new(rlbot_python)
            .args(["-m", "venv", venv_activate_path.to_str().unwrap()])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await?;
        info!("Virtual environment created")
    }

    if is_online {
        info!("Updating rlbot...");
        let update_commands = [
            "@ECHO OFF",
            &format!("call {}", venv_activate_bat.as_path().to_str().unwrap()),
            "python -m pip install --upgrade pip",
            "pip install wheel",
            "pip install eel",
            "pip install --upgrade rlbot_gui rlbot",
        ];
        run_bat(&update_commands.join("\n")).await?;
    } else {
        warn!("It seems you're offline, skipping updates. If this is the first time you're running rlbot, you need to connect to the internet.")
    }

    info!("Starting GUI");
    let update_commands = [
        "@ECHO OFF",
        &format!("call {}", venv_activate_bat.to_str().unwrap()),
        "python -c \"from rlbot_gui import gui; gui.start()\"",
    ];
    run_bat(&update_commands.join("\n")).await?;

    Ok(())
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    if !cfg!(target_os = "windows") {
        panic!("Only windows is supported")
    }
    match realmain().await {
        Ok(_) => {}
        Err(e) => {
            error!("{}", e.to_string());
            print!("Press any key to exit... ");
            stdout().flush().expect("couldn't flush stdout");

            let term = Term::stdout();
            term.read_key().expect("failed to read key");
        }
    }
}
