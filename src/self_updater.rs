use std::{
    env,
    error::Error,
    io::{Cursor, Read},
    path::Path,
};

use reqwest::header::USER_AGENT;
use serde::Deserialize;
use tokio::fs;
use tracing::{info, warn};

use crate::pause;

// github redirects to new repo name/location if this updates
const RELEASE_REPO_OWNER: &str = "swz-git";
const RELEASE_REPO_NAME: &str = "guilauncher";

#[derive(Debug, Deserialize)]
struct Asset {
    // url: String,
    name: String,
    browser_download_url: String,
}

// Example: https://api.github.com/repos/swz-git/guilauncher/releases/latest
#[derive(Debug, Deserialize)]
struct Release {
    // tag_name: String,
    name: String,
    assets: Vec<Asset>,
}

async fn self_update(new_release: &Release) -> Result<(), Box<dyn Error>> {
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

pub async fn check_self_update(force_update: bool) -> Result<bool, Box<dyn Error>> {
    let latest_release_url = format!(
        "https://api.github.com/repos/{RELEASE_REPO_OWNER}/{RELEASE_REPO_NAME}/releases/latest"
    );
    let reqwest_client = reqwest::Client::new();
    let req = match reqwest_client
        .get(latest_release_url)
        .header(USER_AGENT, "rlbot-gui-launcher")
        .send()
        .await
    {
        Ok(x) => x,
        Err(_) => {
            warn!("Couldn't find latest release, self-updating is not available");
            return Ok(false);
        }
    };

    let req_text = &req.text().await?;

    let latest_release: Release = match serde_json::from_str(req_text) {
        Ok(x) => x,
        Err(_) => {
            warn!("Couldn't parse latest release, self-updating is not available");
            return Ok(false);
        }
    };

    let current_version_name = env!("CARGO_PKG_VERSION");
    let latest_version_name = &latest_release.name;

    if current_version_name != latest_version_name {
        info!("Update found, self-updating...");
        self_update(&latest_release).await?;
        return Ok(true);
    } else if force_update {
        info!("Forcing self-update...");
        self_update(&latest_release).await?;
        return Ok(true);
    } else {
        info!("Already using latest version!");
    }

    Ok(false)
}
