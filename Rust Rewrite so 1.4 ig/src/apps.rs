use std::path::Path;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub fn list_apps() -> Vec<(String, String)> {
    #[cfg(windows)]
    {
        vec![
            ("Notepad".to_string(), "notepad.exe".to_string()),
            ("Paint".to_string(), "mspaint.exe".to_string()),
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec![];
    }
    #[cfg(target_os = "linux")]
    {
        vec![];
    }
}

pub fn launch_with(exe: &str, path: &Path) {
    let mut cmd = Command::new(exe);
    cmd.arg(path);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let _ = cmd.spawn();
}

pub fn default_open(path: &Path) {
    #[cfg(windows)]
    {
        let path_str = path.to_string_lossy();
        let _ = Command::new("cmd")
            .args(["/C", "start", "", &path_str])
            .creation_flags(0x08000000)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}
