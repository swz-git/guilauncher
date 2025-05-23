use std::{
    env, fs,
    io::{Cursor, Read},
    path::Path,
};

use anyhow::{anyhow, Context};
use serde::Deserialize;
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

fn self_update(new_release: &Release) -> anyhow::Result<()> {
    let zip_asset = new_release
        .assets
        .iter()
        .find(|r| {
            r.name.contains("guilauncher")
                && Path::new(&r.name)
                    .extension()
                    .map_or(false, |ext| ext.eq_ignore_ascii_case("zip"))
        })
        .context("Couldn't find binary of latest release")?;

    info!("Downloading latest release zip");

    let response = ureq::get(&zip_asset.browser_download_url).call()?;

    let mut zip_bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut zip_bytes)?;

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
        info!("Updating self, PLEASE DO NOT CLOSE THIS WINDOW");
        let temp_bin = Path::join(env::temp_dir().as_path(), "TEMPrlbotguilauncher.exe");
        fs::write(&temp_bin, new_binary)?;
        self_replace::self_replace(&temp_bin)?;
        fs::remove_file(temp_bin)?;
        info!("Done! Please restart this program.");
        pause();
    } else {
        return Err(anyhow!("Couldn't find new binary in zip"));
    }

    Ok(())
}

pub fn check_self_update(force_update: bool) -> anyhow::Result<bool> {
    let latest_release_url = format!(
        "https://api.github.com/repos/{RELEASE_REPO_OWNER}/{RELEASE_REPO_NAME}/releases/latest"
    );
    let Ok(req) = ureq::get(&latest_release_url)
        .header("User-Agent", "rlbot-gui-launcher")
        .call()
    else {
        warn!("Couldn't find latest release, self-updating is not available");
        return Ok(false);
    };

    let req_text = &req.into_body().read_to_string()?;

    let Ok(latest_release) = serde_json::from_str::<Release>(req_text) else {
        warn!("Couldn't parse latest release, self-updating is not available");
        return Ok(false);
    };

    let current_version_name = env!("CARGO_PKG_VERSION");
    let latest_version_name = &latest_release.name;

    if current_version_name != latest_version_name {
        info!("Update found, self-updating...");
        self_update(&latest_release)?;
        return Ok(true);
    } else if force_update {
        info!("Forcing self-update...");
        self_update(&latest_release)?;
        return Ok(true);
    }

    info!("Already using latest version!");
    Ok(false)
}
