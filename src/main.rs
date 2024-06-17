mod self_updater;

use async_compression::tokio::bufread::XzDecoder;
use clap::Parser;
use console::Term;
use directories::BaseDirs;
use self_updater::check_self_update;
use std::{
    env,
    error::Error,
    io::{stdout, Cursor, Write},
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{fs, net::TcpStream, process::Command};
use tokio_stream::StreamExt;
use tracing::{error, info, warn};
use yansi::Paint;

// from https://github.com/indygreg/python-build-standalone/releases/tag/20240415
// originally cpython-3.11.9+20240415-x86_64-pc-windows-msvc-install_only.tar.gz
// decompressed, pdb files removed, recompressed as xz
const PYTHON311_COMPRESSED: &[u8] = include_bytes!("../assets/cpython-3.11.9-custom-rlbot.tar.xz");

async fn is_online() -> bool {
    TcpStream::connect("pypi.org:80").await.is_ok()
}

fn pause() {
    print!("Press any key to exit... ");
    stdout().flush().expect("couldn't flush stdout");

    let term = Term::stdout();
    term.read_key().expect("failed to read key");
}

async fn clear_pip_cache(base_dirs: BaseDirs) -> Result<(), Box<dyn Error>> {
    let cache_dirs = [
        base_dirs.data_local_dir().join("pip/cache"),
        base_dirs.data_local_dir().join("uv/cache"),
    ];
    for dir in cache_dirs {
        fs::remove_dir_all(&dir).await?;
    }
    Ok(())
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

    /// Clears cache of pip (and uv)
    #[arg(short, long, default_value_t = false)]
    clear_pip_cache: bool,

    // Run as if offline
    #[arg(short, long, default_value_t = false)]
    offline: bool,
}

async fn realmain() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let rlbot_ascii_art = include_str!("../assets/rlbot-ascii-art.txt");
    println!("{}\n", rlbot_ascii_art.green());

    info!("Checking for internet connection...");

    let is_online = is_online().await && !args.offline;

    info!("Is online: {is_online}");

    // Check for self update
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

    let python_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/Python311");

    let python_exe = python_dir.join("python.exe");

    // Clear python cache if told to do so
    if args.clear_pip_cache {
        info!("Clearing package cache");
        clear_pip_cache(base_dirs).await?;
    }

    // Add paths that antiviruses may remove here
    // If any of these don't exist, we reinstall python
    let crucial_python_components = [&python_exe];
    let crucial_python_components_installed = crucial_python_components
        .iter()
        .fold(true, |acc, path| path.exists() && acc);

    if python_dir.exists() && args.python_reinstall {
        info!("Removing current python install...");
        fs::remove_dir_all(&python_dir).await?
    } else if python_dir.exists() && !crucial_python_components_installed {
        info!("Broken python detected, reinstalling");
        fs::remove_dir_all(&python_dir).await?
    }

    if !crucial_python_components_installed || args.python_reinstall {
        info!("Python not found, installing...");
        let decoder = XzDecoder::new(Cursor::new(PYTHON311_COMPRESSED));
        let mut tar = tokio_tar::Archive::new(decoder);

        // tar.unpack(&python_dir).await?;
        // the code above results in RLBotGUIX/Python311/python/[PYTHONFILES]
        // because of this, we do the following:

        let mut entries = tar.entries()?;
        while let Some(Ok(mut entry)) = entries.next().await {
            let path_in_tar = entry.path()?;
            // all paths start with `python/`, we wanna remove that
            let path_in_tar_without_parent: PathBuf = path_in_tar.components().skip(1).collect();
            entry
                .unpack(python_dir.join(path_in_tar_without_parent))
                .await?;
        }
        info!("Python installed");
    } else {
        info!("Python install found, continuing");
    }

    macro_rules! python_command {
        ($args:expr) => {{
            let exit_status = Command::new(python_exe.to_str().unwrap())
                .args($args)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .current_dir(env::temp_dir())
                // The below is needed because uv refuses to run without it
                .env("VIRTUAL_ENV", python_dir.to_str().unwrap())
                .status()
                .await?;
            if !exit_status.success() {
                Err("Command failed")?
            }
        }};
    }

    if is_online {
        info!("Updating rlbot...");

        // Make sure pip is up to date
        python_command!(&[
            "-m",
            "pip",
            "install",
            "-U",
            "pip",
            "--no-warn-script-location",
        ]);
        // Make sure uv is up to date
        python_command!(&[
            "-m",
            "pip",
            "install",
            "uv==0.2.11",
            "--no-warn-script-location",
        ]);
        // Install rlbot and deps with uv
        python_command!(&[
            "-m",
            "uv",
            "pip",
            "install",
            "-U",
            "setuptools",
            "wheel",
            "gevent",
            "eel",
            "rlbot_gui",
            "rlbot",
            "numpy==1.*" // numpy is an indirect dependency and 2.* breaks a lot of things
        ]);
    } else {
        warn!("It seems you're offline, skipping updates. If this is the first time you're running rlbot, you need to connect to the internet.");
    }

    info!("Starting GUI");
    python_command!(&["-c", "from rlbot_gui import gui; gui.start()"]);

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
