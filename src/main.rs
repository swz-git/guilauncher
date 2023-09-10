#[macro_use]
extern crate log;

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

const PYTHON37_ZIP_URL: &str = "https://github.com/RLBot/RLBotGUI/raw/master/alternative-install/python-3.7.9-custom-amd64.zip";

const RELEASE_REPO_OWNER: &str = "swz-git";
const RELEASE_REPO_NAME: &str = "guilauncher";

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

async fn realmain() -> Result<(), Box<dyn Error>> {
    let rlbot_banner = include_str!("../assets/rlbot-banner.txt");
    println!("{}\n", rlbot_banner.green());
    info!("Checking for internet connection...");

    let is_online = is_online();

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
    let python_install_dir = Path::join(base_dirs.data_local_dir(), "RLBotGUIX/Python37");
    if !python_install_dir.exists() {
        info!("Python not found, installing...");
        if !is_online {
            Err("RLBot needs python to function and can't download it since you're offline. Please connect to the internet and try again.")?
        }
        let zip = reqwest::get(PYTHON37_ZIP_URL).await?.bytes().await?;
        zip_extract::extract(Cursor::new(zip), &python_install_dir, true)?;
        info!("Python installed")
    } else {
        info!("Python install found, continuing")
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
    } else {
        info!("RLBot virtual python environment found, continuing")
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
            pause();
        }
    }
}
