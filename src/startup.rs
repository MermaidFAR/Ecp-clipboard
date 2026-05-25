use std::env;
use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "Ecp Clipboard";

#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let exe_path = env::current_exe()
        .map_err(|error| format!("无法获取当前程序路径: {error}"))?
        .display()
        .to_string();

    let status = if enabled {
        Command::new("reg")
            .args([
                "add",
                RUN_KEY,
                "/v",
                APP_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{exe_path}\""),
                "/f",
            ])
            .status()
    } else {
        Command::new("reg")
            .args(["delete", RUN_KEY, "/v", APP_NAME, "/f"])
            .status()
    }
    .map_err(|error| format!("无法调用 reg.exe: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("reg.exe 返回失败状态: {status}"))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn set_enabled(_enabled: bool) -> Result<(), String> {
    Err(String::from("当前平台暂不支持开机自启"))
}
