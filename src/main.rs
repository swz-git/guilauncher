mod self_updater;

use clap::Parser;
use console::Term;
use directories::BaseDirs;
use self_updater::check_self_update;
use std::{
    error::Error,
    io::{stdout, Cursor, Write},
    path::Path,
    process::Stdio,
};
use tokio::{fs, net::TcpStream, process::Command};
use tracing::{error, info, warn};
use yansi::Paint;

// const PYTHON37_ZIP_URL: &str = "https://github.com/RLBot/RLBotGUI/raw/master/alternative-install/python-3.7.9-custom-amd64.zip";
// const PYTHON311_ZIP_URL: &str = "https://github.com/RLBot/gui-installer/raw/master/RLBotGUIX%20Installer/python-3.11.6-custom-amd64.zip";
const PYTHON311_ZIP_DATA: &[u8] = include_bytes!("../assets/python-3.11.6-custom-amd64.zip");

async fn is_online() -> bool {
    TcpStream::connect("pypi.org:80").await.is_ok()
}

async fn run_command(cmd: &str, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let exit_status = Command::new(cmd)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    if !exit_status.success() {
        Err("Command failed")?
    }
    Ok(())
}

fn pause() {
    print!("Press any key to exit... ");
    stdout().flush().expect("couldn't flush stdout");

    let term = Term::stdout();
    term.read_key().expect("failed to read key");
}

/// Launcher for RLBotGUI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Force self-update
    #[arg(short, long, default_value_t = false)]
    force_self_update: bool,

    /// Reinstall python
    #[arg(short, long, default_value_t = false)]
    python_reinstall: bool,

    // Run as if offline
    #[arg(short, long, default_value_t = false)]
    offline: bool,
}

async fn realmain() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let rlbot_banner = include_str!("../assets/rlbot-banner.txt");
    println!("{}\n", rlbot_banner.green());
    info!("Checking for internet connection...");

    let is_online = is_online().await && !args.offline;

    info!("Is online: {is_online}");

    // Check for self update
    // TODO: add clap flag for forced self-update
    if is_online {
        info!("Checking for self-updates...");
        let self_updated = match check_self_update(args.force_self_update).await {
            Ok(self_updated) => self_updated,
            Err(e) => {
                error!("{}", e.to_string());
                warn!("Self-update failed due to previous error. Skipping self-update and running anyways");
                false
            }
        };

        if self_updated {
            return Ok(());
        }
    } else {
        warn!("Not checking for updates because no internet connection was found");
    }

    let base_dirs = BaseDirs::new().ok_or("Couldn't get BaseDirs")?;

    // Check for RLBotGUIX path
    let rlbotgui_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX");
    if !rlbotgui_dir.exists() {
        fs::create_dir_all(rlbotgui_dir).await?;
    }

    // Check for python install
    let python37_install_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/Python37");
    if python37_install_dir.exists() {
        info!("Legacy python37 found");
    }

    let python_install_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/Python311");

    let rlbot_python = python_install_dir.join("python.exe");
    let rlbot_pip = python_install_dir.join("Scripts/pip.exe");

    let crucial_python_components = [&rlbot_python, &rlbot_pip];

    let crucial_python_components_installed = crucial_python_components
        .iter()
        .fold(true, |acc, path| path.exists() && acc);

    if args.python_reinstall {
        info!("Removing current python install...");
        fs::remove_dir_all(&python_install_dir).await?;
    }

    if !crucial_python_components_installed || args.python_reinstall {
        info!("Python not found, installing...");
        zip_extract::extract(Cursor::new(PYTHON311_ZIP_DATA), &python_install_dir, true)?;
        info!("Python installed");
    } else {
        info!("Python install found, continuing");
    }

    if is_online {
        info!("Updating rlbot...");
        run_command(
            rlbot_python.to_str().unwrap(),
            &[
                "-m",
                "pip",
                "install",
                "-U",
                "pip",
                "--no-warn-script-location",
            ],
        )
        .await?;
        run_command(
            rlbot_python.to_str().unwrap(),
            &[
                "-m",
                "pip",
                "install",
                "-U",
                "pip",
                "--no-warn-script-location",
            ],
        )
        .await?;
        run_command(
            rlbot_pip.to_str().unwrap(),
            &[
                "install",
                "-U",
                "setuptools",
                "wheel",
                "--no-warn-script-location",
            ],
        )
        .await?;
        run_command(
            rlbot_pip.to_str().unwrap(),
            &["install", "-U", "gevent", "--no-warn-script-location"],
        )
        .await?;
        run_command(
            rlbot_pip.to_str().unwrap(),
            &["install", "-U", "eel", "--no-warn-script-location"],
        )
        .await?;
        run_command(
            rlbot_pip.to_str().unwrap(),
            &[
                "install",
                "-U",
                "rlbot_gui",
                "rlbot",
                "--no-warn-script-location",
            ],
        )
        .await?;
    } else {
        warn!("It seems you're offline, skipping updates. If this is the first time you're running rlbot, you need to connect to the internet.");
    }

    info!("Starting GUI");
    run_command(
        rlbot_python.to_str().unwrap(),
        &["-c", "from rlbot_gui import gui; gui.start()"],
    )
    .await?;

    Ok(())
}

#[cfg(not(windows))]
compile_error!("Only windows is supported");

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    if let Err(e) = realmain().await {
        error!("{}", e.to_string());
        info!("If you need help, join our discord! https://rlbot.org/discord/");
        pause();
    }
}
