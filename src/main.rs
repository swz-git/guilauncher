#[macro_use]
extern crate log;

use clap::Parser;
use console::Term;
use directories::BaseDirs;
use env_logger::Env;
use octocrab::models::repos::Release;
use std::{
    env,
    error::Error,
    io::{stdout, Cursor, Read, Write},
    net::TcpStream,
    path::Path,
    process::Stdio,
};
use tokio::{fs, process::Command};
use yansi::Paint;

// const PYTHON37_ZIP_URL: &str = "https://github.com/RLBot/RLBotGUI/raw/master/alternative-install/python-3.7.9-custom-amd64.zip";
const PYTHON311_ZIP_URL: &str = "https://github.com/RLBot/gui-installer/raw/master/RLBotGUIX%20Installer/python-3.11.6-custom-amd64.zip";

const RELEASE_REPO_OWNER: &str = "swz-git";
const RELEASE_REPO_NAME: &str = "guilauncher";

fn is_online() -> bool {
    match TcpStream::connect("pypi.org:80") {
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

fn pause() {
    print!("Press any key to exit... ");
    stdout().flush().expect("couldn't flush stdout");

    let term = Term::stdout();
    term.read_key().expect("failed to read key");
}

// TODO: Check checksum
async fn self_update(new_release: Release) -> Result<(), Box<dyn Error>> {
    let zip_asset = new_release
        .assets
        .iter()
        .find(|r| r.name.contains("guilauncher") && r.name.ends_with(".zip"))
        .expect("Couldn't find binary of latest release");

    info!("Downloading latest release zip");

    let zip_bytes = reqwest::get(zip_asset.browser_download_url.to_string())
        .await?
        .bytes()
        .await?;

    info!("Extracting executable");
    let mut zip = zip::ZipArchive::new(Cursor::new(zip_bytes))?;

    let mut maybe_new_binary = None;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        if file.name().ends_with("exe") {
            let mut buf: Vec<u8> = vec![];
            file.read_to_end(&mut buf)?;
            maybe_new_binary = Some(buf);

            break;
        }
    }

    if let Some(new_binary) = maybe_new_binary {
        info!("Updating self");
        let temp_bin = Path::join(env::temp_dir().as_path(), "TEMPrlbotguilauncher.exe");
        fs::write(&temp_bin, new_binary).await?;
        self_replace::self_replace(&temp_bin)?;
        fs::remove_file(temp_bin).await?;
        info!("Done! Please restart this program.");
        pause();
    } else {
        Err("Couldn't find new binary in zip")?
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

    // Run as if offline
    #[arg(short, long, default_value_t = false)]
    offline: bool,
}

async fn realmain() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let rlbot_banner = include_str!("../assets/rlbot-banner.txt");
    println!("{}\n", rlbot_banner.green());
    info!("Checking for internet connection...");

    let is_online = is_online() && !args.offline;

    info!("Is online: {is_online}");

    // Check for self update
    // TODO: add clap flag for forced self-update
    if is_online {
        info!("Checking for self-updates...");

        let crab = octocrab::instance();
        let repo = crab.repos(RELEASE_REPO_OWNER, RELEASE_REPO_NAME);

        let current_version_name = env!("CARGO_PKG_VERSION");
        let latest_release = repo.releases().get_latest().await?;

        if let Some(latest_version_name) = &latest_release.name {
            if current_version_name != latest_version_name {
                info!("Update found, self-updating...");
                return self_update(latest_release).await;
            } else if args.force_self_update {
                info!("Forcing self-update...");
                return self_update(latest_release).await;
            } else {
                info!("Already using latest version!")
            }
        } else {
            warn!("Couldn't find latest release, self-updating is not available")
        }
    } else {
        warn!("Not checking for updates because no internet connection was found")
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
        info!("Legacy python37 found")
    }

    let python_install_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/Python311");

    let rlbot_python = python_install_dir.join("python.exe");
    let rlbot_pip = python_install_dir.join("Scripts/pip.exe");

    let crucial_python_components = vec![&rlbot_python, &rlbot_pip];

    let crucial_python_components_installed = crucial_python_components
        .iter()
        .fold(true, |acc, path| path.exists() && acc);

    if !crucial_python_components_installed {
        if args.python_reinstall {
            info!("Removing current python install...");
            fs::remove_dir_all(&python_install_dir).await?;
        }

        info!("Python not found, installing...");
        if !is_online {
            Err("RLBot needs python to function and can't download it since you're offline. Please connect to the internet and try again.")?
        }
        let zip = reqwest::get(PYTHON311_ZIP_URL).await?.bytes().await?;
        zip_extract::extract(Cursor::new(zip), &python_install_dir, true)?;
        info!("Python installed")
    } else {
        info!("Python install found, continuing")
    }

    if is_online {
        info!("Updating rlbot...");
        let update_script = format!(
            r#"@ECHO OFF
            {0} -m pip install -U pip --no-warn-script-location
            {1} install -U setuptools wheel --no-warn-script-location
            {1} install -U gevent --no-warn-script-location
            {1} install -U eel --no-warn-script-location
            {1} install -U rlbot_gui rlbot --no-warn-script-location"#,
            rlbot_python.to_str().unwrap(),
            rlbot_pip.to_str().unwrap()
        );
        run_bat(&update_script).await?;
    } else {
        warn!("It seems you're offline, skipping updates. If this is the first time you're running rlbot, you need to connect to the internet.")
    }

    info!("Starting GUI");
    let launch_script = format!(
        r#"@ECHO OFF
        {0} -c "from rlbot_gui import gui; gui.start()""#,
        rlbot_python.to_str().unwrap()
    );
    run_bat(&launch_script).await?;

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
            pause();
        }
    }
}
