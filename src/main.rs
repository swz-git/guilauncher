mod self_updater;

use anyhow::{anyhow, Context};
use clap::Parser;
use console::Term;
use directories::BaseDirs;
use indicatif::{ProgressBar, ProgressStyle};
use self_updater::check_self_update;
use std::{
    env, fs,
    io::{stdout, Cursor, Read, Seek, SeekFrom, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tracing::{error, info, warn};
use xz::bufread::XzDecoder;
use yansi::Paint;

// from https://github.com/indygreg/python-build-standalone/releases/tag/20240415
// originally cpython-3.11.9+20240415-x86_64-pc-windows-msvc-install_only.tar.gz
// decompressed, pdb files removed, recompressed as xz
const PYTHON311_COMPRESSED: &[u8] = include_bytes!("../assets/cpython-3.11.9-custom-rlbot.tar.xz");

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

fn realmain() -> anyhow::Result<()> {
    let args = Args::parse();
    let rlbot_ascii_art = include_str!("../assets/rlbot-ascii-art.txt");
    println!("{}\n", rlbot_ascii_art.green());

    info!("Checking for internet connection...");

    let is_online = is_online() && !args.offline;

    info!("Is online: {is_online}");

    // Check for self update
    if is_online {
        info!("Checking for self-updates...");
        let self_updated = match check_self_update(args.force_self_update) {
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

    let base_dirs = BaseDirs::new().ok_or(anyhow!("Couldn't get BaseDirs"))?;

    // Check for RLBotGUIX path
    let rlbotgui_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX");
    if !rlbotgui_dir.exists() {
        fs::create_dir_all(rlbotgui_dir)?;
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
        clear_pip_cache(base_dirs).context("Couldn't clear pip cache")?;
    }

    // Add paths that antiviruses may remove here
    // If any of these don't exist, we reinstall python
    let crucial_python_components = [&python_exe];
    let crucial_python_components_installed = crucial_python_components
        .iter()
        .fold(true, |acc, path| path.exists() && acc);

    if python_dir.exists() && args.python_reinstall {
        info!("Removing current python install...");
        fs::remove_dir_all(&python_dir)?
    } else if python_dir.exists() && !crucial_python_components_installed {
        info!("Broken python detected, reinstalling");
        fs::remove_dir_all(&python_dir)?
    }

    if !crucial_python_components_installed || args.python_reinstall {
        info!("Python not found, installing...");
        install_python(&python_dir).context("Python install failed")?;
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
                .status()?;
            if !exit_status.success() {
                Err(anyhow!("Command failed"))?
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
            "uv==0.5.4",
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
            "numpy==1.*", // numpy is an indirect dependency and 2.* breaks a lot of things
            "websockets==12"  // websockets 14 breaks rlbot "no running event loop"
        ]);
    } else {
        warn!("It seems you're offline, skipping updates. If this is the first time you're running rlbot, you need to connect to the internet.");
    }

    info!("Starting GUI");
    python_command!(&["-c", "from rlbot_gui import gui; gui.start()"]);

    Ok(())
}

fn is_online() -> bool {
    TcpStream::connect("pypi.org:80").is_ok()
}

fn pause() {
    print!("Press any key to exit... ");
    stdout().flush().expect("couldn't flush stdout");

    let term = Term::stdout();
    term.read_key().expect("failed to read key");
}

fn clear_pip_cache(base_dirs: BaseDirs) -> anyhow::Result<()> {
    let cache_dirs = [
        base_dirs.data_local_dir().join("pip/cache"),
        base_dirs.data_local_dir().join("uv/cache"),
    ];
    for dir in cache_dirs {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

fn install_python(dir: &Path) -> anyhow::Result<()> {
    let mut decoder = XzDecoder::new(Cursor::new(PYTHON311_COMPRESSED));
    let decoded = {
        let mut buf = Vec::new();
        decoder
            .read_to_end(&mut buf)
            .context("XzDecoder read failed")?;
        buf
    };
    let mut tar_archive = tar::Archive::new(Cursor::new(decoded));

    let pb = ProgressBar::new(tar_archive.entries_with_seek()?.count() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {elapsed_precise} [{bar:15.cyan/blue}] {pos}/{len} ~{eta} left",
        )
        .unwrap()
        .tick_chars("-\\|/✓"),
    );

    // recreate archive
    let mut tar_archive = tar::Archive::new({
        let mut cursor = tar_archive.into_inner();
        cursor.seek(SeekFrom::Start(0)).unwrap();
        cursor
    });

    // tar.unpack(&python_dir).await?;
    // the code above results in RLBotGUIX/Python311/python/[PYTHONFILES]
    // because of this, we do the following:

    let mut entries = tar_archive.entries()?;
    while let Some(Ok(mut entry)) = entries.next() {
        let path_in_tar = entry.path()?;
        // all paths start with `python/`, we wanna remove that
        let path_in_tar_without_parent: PathBuf = path_in_tar.components().skip(1).collect();
        entry.unpack(dir.join(path_in_tar_without_parent))?;
        pb.set_position(pb.position() + 1);
    }
    pb.finish_with_message("done");
    Ok(())
}

#[cfg(not(windows))]
compile_error!("Only windows is supported");

fn main() {
    tracing_subscriber::fmt::init();

    if let Err(e) = realmain() {
        error!("{}", e.to_string());
        info!("If you need help, join our discord! https://rlbot.org/discord/");
        pause();
    }
}
