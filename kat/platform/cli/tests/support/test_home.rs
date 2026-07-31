#[cfg(windows)]
use std::fs;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn configure(command: &mut Command, root: &Path) {
    command.env_remove("KAT_DATA_HOME");

    #[cfg(not(windows))]
    command
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("xdg-data"));

    #[cfg(windows)]
    {
        let profile = root.join("profile");
        let roaming = profile.join("AppData").join("Roaming");
        let local = profile.join("AppData").join("Local");
        fs::create_dir_all(&roaming).expect("create test Roaming AppData");
        fs::create_dir_all(&local).expect("create test Local AppData");
        command
            .env("USERPROFILE", &profile)
            .env("APPDATA", &roaming)
            .env("LOCALAPPDATA", &local);
    }
}

pub fn data_home(root: &Path) -> PathBuf {
    #[cfg(not(windows))]
    {
        root.join("xdg-data").join("kat")
    }

    #[cfg(windows)]
    {
        root.join("profile")
            .join("AppData")
            .join("Roaming")
            .join("KAT")
            .join("data")
    }
}
