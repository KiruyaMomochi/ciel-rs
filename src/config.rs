//! This module contains configuration files related APIs

use crate::common::CURRENT_CIEL_VERSION;
use crate::info;
use anyhow::{anyhow, Result};
use console::{style, user_attended};
use dialoguer::{theme::ColorfulTheme, Confirm, Editor, Input};
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, path::Path};
use std::{
    fs,
    io::{Read, Write},
};

const DEFAULT_CONFIG_LOCATION: &str = ".ciel/data/config.toml";
const DEFAULT_APT_SOURCE: &str = "deb https://repo.aosc.io/debs/ stable main";
const DEFAULT_AB3_CONFIG_LOCATION: &str = "usr/lib/autobuild3/etc/autobuild/ab3cfg.sh";
const DEFAULT_APT_LIST_LOCATION: &str = "etc/apt/sources.list";
const DEFAULT_RESOLV_LOCATION: &str = "etc/systemd/resolved.conf";
const DEFAULT_ACBS_CONFIG: &str = "etc/acbs/forest.conf";

#[derive(Debug, Serialize, Deserialize)]
pub struct CielConfig {
    version: usize,
    maintainer: String,
    dnssec: bool,
    apt_sources: String,
    pub local_repo: bool,
    pub local_sources: bool,
    #[serde(rename = "nspawn-extra-options")]
    pub extra_options: Vec<String>,
    #[serde(rename = "branch-exclusive-output")]
    pub sep_mount: bool,
    #[serde(rename = "volatile-mount", default)]
    pub volatile_mount: bool,
}

impl CielConfig {
    pub fn save_config(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    pub fn load_config(data: &str) -> Result<CielConfig> {
        Ok(toml::from_str(data)?)
    }
}

impl Default for CielConfig {
    fn default() -> Self {
        CielConfig {
            version: CURRENT_CIEL_VERSION,
            maintainer: "Bot <null@aosc.io>".to_string(),
            dnssec: false,
            apt_sources: DEFAULT_APT_SOURCE.to_string(),
            local_repo: true,
            local_sources: true,
            extra_options: Vec::new(),
            sep_mount: true,
            volatile_mount: false,
        }
    }
}

#[allow(clippy::ptr_arg)]
fn validate_maintainer(maintainer: &String) -> Result<(), String> {
    let mut lt = false; // "<"
    let mut gt = false; // ">"
    let mut at = false; // "@"
    let mut name = false;
    let mut nbsp = false; // space
                          // A simple FSM to match the states
    for c in maintainer.as_bytes() {
        match *c {
            b'<' => {
                if !nbsp {
                    return Err("Please enter a name.".to_owned());
                }
                lt = true;
            }
            b'>' => {
                if !lt {
                    return Err("Invalid format.".to_owned());
                }
                gt = true;
            }
            b'@' => {
                if !lt || gt {
                    return Err("Invalid format.".to_owned());
                }
                at = true;
            }
            b' ' | b'\t' => {
                if !name {
                    return Err("Please enter a name.".to_owned());
                }
                nbsp = true;
            }
            _ => {
                if !nbsp {
                    name = true;
                    continue;
                }
            }
        }
    }

    if name && gt && lt && at {
        return Ok(());
    }

    Err("Invalid format.".to_owned())
}

#[inline]
fn create_parent_dir(path: &Path) -> Result<()> {
    let path = path
        .parent()
        .ok_or_else(|| anyhow!("Parent directory is root."))?;
    fs::create_dir_all(path)?;

    Ok(())
}

#[inline]
fn get_default_editor() -> OsString {
    if let Some(prog) = std::env::var_os("VISUAL") {
        return prog;
    }
    if let Some(prog) = std::env::var_os("EDITOR") {
        return prog;
    }
    if let Ok(editor) = which::which("editor") {
        return editor.as_os_str().to_os_string();
    }

    "nano".into()
}

/// Shows a series of prompts to let the user select the configurations
pub fn ask_for_config(config: Option<CielConfig>) -> Result<CielConfig> {
    let mut config = config.unwrap_or_default();
    if !user_attended() {
        info!("Not controlled by an user. Default values are used.");
        return Ok(config);
    }
    let theme = ColorfulTheme::default();
    config.maintainer = Input::<String>::with_theme(&theme)
        .with_prompt("Maintainer Information")
        .default(config.maintainer)
        .validate_with(validate_maintainer)
        .interact_text()?;
    config.dnssec = Confirm::with_theme(&theme)
        .with_prompt("Enable DNSSEC")
        .default(config.dnssec)
        .interact()?;
    let edit_source = Confirm::with_theme(&theme)
        .with_prompt("Edit sources.list")
        .default(false)
        .interact()?;
    if edit_source {
        config.apt_sources = Editor::new()
            .executable(get_default_editor())
            .extension(".list")
            .edit(if config.apt_sources.is_empty() {
                DEFAULT_APT_SOURCE
            } else {
                &config.apt_sources
            })?
            .unwrap_or_else(|| DEFAULT_APT_SOURCE.to_owned());
    }
    config.local_sources = Confirm::with_theme(&theme)
        .with_prompt("Enable local sources caching")
        .default(config.local_sources)
        .interact()?;
    config.local_repo = Confirm::with_theme(&theme)
        .with_prompt("Enable local packages repository")
        .default(config.local_repo)
        .interact()?;
    config.sep_mount = Confirm::with_theme(&theme)
        .with_prompt("Use different OUTPUT dir for different branches")
        .default(config.sep_mount)
        .interact()?;
    config.volatile_mount = Confirm::with_theme(&theme)
        .with_prompt("Use volatile mode for filesystem operations")
        .default(config.volatile_mount)
        .interact()?;

    Ok(config)
}

/// Reads the configuration file from the current workspace
pub fn read_config() -> Result<CielConfig> {
    let mut f = std::fs::File::open(DEFAULT_CONFIG_LOCATION)?;
    let mut data = String::new();
    f.read_to_string(&mut data)?;

    CielConfig::load_config(&data)
}

/// Applies the given configuration (th configuration itself will not be saved to the disk)
pub fn apply_config<P: AsRef<Path>>(root: P, config: &CielConfig) -> Result<()> {
    // write maintainer information
    let rootfs = root.as_ref();
    let mut config_path = rootfs.to_owned();
    config_path.push(DEFAULT_AB3_CONFIG_LOCATION);
    create_parent_dir(&config_path)?;
    let mut f = std::fs::File::create(config_path)?;
    f.write_all(
        format!(
            "#!/bin/bash\nABMPM=dpkg\nABAPMS=\nABINSTALL=dpkg\nMTER=\"{}\"",
            config.maintainer
        )
        .as_bytes(),
    )?;
    // write sources.list
    if !config.apt_sources.is_empty() {
        let mut apt_list_path = rootfs.to_owned();
        apt_list_path.push(DEFAULT_APT_LIST_LOCATION);
        create_parent_dir(&apt_list_path)?;
        let mut f = std::fs::File::create(apt_list_path)?;
        f.write_all(config.apt_sources.as_bytes())?;
    }
    // write DNSSEC configuration
    if !config.dnssec {
        let mut resolv_path = rootfs.to_owned();
        resolv_path.push(DEFAULT_RESOLV_LOCATION);
        create_parent_dir(&resolv_path)?;
        let mut f = std::fs::File::create(resolv_path)?;
        f.write_all(b"[Resolve]\nDNSSEC=no\n")?;
    }
    // write acbs configuration
    let mut acbs_path = rootfs.to_owned();
    acbs_path.push(DEFAULT_ACBS_CONFIG);
    create_parent_dir(&acbs_path)?;
    let mut f = std::fs::File::create(acbs_path)?;
    f.write_all(b"[default]\nlocation = /tree/\n")?;

    Ok(())
}

#[test]
fn test_validate_maintainer() {
    assert_eq!(
        validate_maintainer(&"test <aosc@aosc.io>".to_owned()),
        Ok(())
    );
    assert_eq!(
        validate_maintainer(&"test <aosc@aosc.io;".to_owned()),
        Err("Invalid format.".to_owned())
    );
}
