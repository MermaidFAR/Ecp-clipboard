#[cfg(target_os = "windows")]
pub fn configure(enabled: bool) -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (advanced_key, _) = hkcu
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced")
        .map_err(|error| format!("无法打开 Explorer Advanced 注册表项: {error}"))?;

    let current = advanced_key
        .get_value::<String, _>("DisabledHotkeys")
        .unwrap_or_default();
    let current_upper = current.to_uppercase();

    if enabled {
        if !current_upper.contains('V') {
            advanced_key
                .set_value("DisabledHotkeys", &format!("{current}V"))
                .map_err(|error| format!("无法禁用系统 Win+V: {error}"))?;
        }
    } else if current_upper.contains('V') {
        let cleaned = current_upper.replace('V', "");
        if cleaned.is_empty() {
            let _ = advanced_key.delete_value("DisabledHotkeys");
        } else {
            advanced_key
                .set_value("DisabledHotkeys", &cleaned)
                .map_err(|error| format!("无法恢复系统 Win+V: {error}"))?;
        }
    }

    let (clipboard_key, _) = hkcu
        .create_subkey("Software\\Microsoft\\Clipboard")
        .map_err(|error| format!("无法打开 Clipboard 注册表项: {error}"))?;
    let value: u32 = if enabled { 0 } else { 1 };
    clipboard_key
        .set_value("EnableClipboardHistory", &value)
        .map_err(|error| format!("无法配置系统剪贴板历史: {error}"))?;
    clipboard_key
        .set_value("EnableCloudClipboard", &value)
        .map_err(|error| format!("无法配置云剪贴板: {error}"))?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn configure(_enabled: bool) -> Result<(), String> {
    Ok(())
}
