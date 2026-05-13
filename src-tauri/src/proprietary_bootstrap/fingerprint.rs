use sha2::{Digest, Sha256};

pub(crate) fn build_device_fingerprint() -> String {
    let platform = std::env::consts::OS;
    let source = fingerprint_source();
    let digest = hex::encode(Sha256::digest(source.as_bytes()));
    format!("v1:{platform}:{digest}")
}

fn fingerprint_source() -> String {
    let mut parts = vec![
        format!("os={}", std::env::consts::OS),
        format!("arch={}", std::env::consts::ARCH),
    ];

    if let Some(hostname) = read_hostname() {
        parts.push(format!("host={hostname}"));
    }

    if let Some(stable_config_suffix) =
        stable_config_suffix(&crate::config::get_app_config_dir())
    {
        parts.push(format!("config={stable_config_suffix}"));
    }

    if let Some(machine_id) = read_machine_id() {
        parts.push(format!("machine={machine_id}"));
    }

    parts.join("\n")
}

fn read_hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(read_hostname_command)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn stable_config_suffix(path: &std::path::Path) -> Option<String> {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_string))
        .collect::<Vec<_>>();
    let marker_index = components.iter().position(|part| part == ".cc-switch")?;
    let suffix = components
        .into_iter()
        .skip(marker_index + 1)
        .collect::<Vec<_>>()
        .join("/");

    if suffix.is_empty() {
        Some("default".to_string())
    } else {
        Some(suffix)
    }
}

#[cfg(not(target_os = "windows"))]
fn read_hostname_command() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

#[cfg(target_os = "windows")]
fn read_hostname_command() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn read_machine_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "windows")]
fn read_machine_id() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    hklm.open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        .ok()
        .and_then(|key| key.get_value::<String, _>("MachineGuid").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "macos")]
fn read_machine_id() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().find_map(|line| {
        if !line.contains("IOPlatformUUID") {
            return None;
        }
        let (_, raw) = line.split_once('=')?;
        Some(raw)
            .map(str::trim)
            .map(|value| value.trim_matches('"'))
            .map(str::trim)
            .map(str::to_string)
            .filter(|value| !value.is_empty())
    })
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn read_machine_id() -> Option<String> {
    None
}
