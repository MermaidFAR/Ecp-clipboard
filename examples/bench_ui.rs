//! Isolated GPUI startup benchmark. Never launches the background executable.
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ecp_clipboard::config::AppConfig;
use ecp_clipboard::db::{Database, EntryKind};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dataset = args.get(1).map(String::as_str).unwrap_or("empty");
    let runs: usize = args
        .get(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!("ecp-bench-{}-{stamp}", std::process::id()));
    let data = root.join("data");
    let config = root.join("config");
    fs::create_dir_all(&data)?;
    fs::create_dir_all(&config)?;
    let settings = AppConfig {
        max_history: 2000,
        ..AppConfig::default()
    };
    fs::write(config.join("settings.json"), serde_json::to_vec(&settings)?)?;
    let db_path = data.join("clipboard.sqlite3");
    seed(&db_path, dataset)?;
    let disk_mb = directory_bytes(&data)? as f64 / 1024. / 1024.;

    let ui = std::env::current_exe()?
        .parent()
        .ok_or("missing examples directory")?
        .parent()
        .ok_or("missing release directory")?
        .join("ecp-ui.exe");
    if !ui.exists() {
        return Err(format!("missing UI executable: {}", ui.display()).into());
    }
    let ready = root.join("ready");
    let mut samples = Vec::new();
    for run in 0..runs {
        let _ = fs::remove_file(&ready);
        let start = Instant::now();
        let mut child = Command::new(&ui)
            .env("ECP_DATA_DIR", &data)
            .env("ECP_CONFIG_DIR", &config)
            .env("ECP_UI_READY_FILE", &ready)
            .spawn()?;
        let completed = loop {
            if ready.exists() {
                break true;
            }
            if let Some(status) = child.try_wait()? {
                eprintln!("UI exited before first frame: {status}");
                break false;
            }
            if start.elapsed() > Duration::from_secs(10) {
                break false;
            }
            thread::sleep(Duration::from_millis(2));
        };
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
        thread::sleep(Duration::from_millis(500));
        let peak_mb = peak_working_set_mb(&child);
        let _ = child.kill();
        let _ = child.wait();
        if !completed {
            return Err(format!(
                "UI did not become interactive; benchmark data: {}",
                root.display()
            )
            .into());
        }
        println!(
            "run={} ready_ms={elapsed_ms:.1} peak_mb={peak_mb:.1}",
            run + 1
        );
        samples.push(elapsed_ms);
        thread::sleep(Duration::from_millis(100));
    }
    let first_run = samples.first().copied().unwrap_or_default();
    let mut warm = if samples.len() > 1 {
        samples[1..].to_vec()
    } else {
        samples.clone()
    };
    warm.sort_by(f64::total_cmp);
    let p50 = warm[((warm.len() - 1) as f64 * 0.50).ceil() as usize];
    let p95 = warm[((warm.len() - 1) as f64 * 0.95).ceil() as usize];
    println!(
        "dataset={dataset} runs={runs} first_run_ms={first_run:.1} warm_p50_ms={p50:.1} warm_p95_ms={p95:.1} disk_mb={disk_mb:.1} data={}",
        root.display()
    );
    Ok(())
}

fn directory_bytes(root: &Path) -> Result<u64, Box<dyn Error>> {
    let mut bytes = 0;
    for item in fs::read_dir(root)? {
        let item = item?;
        let metadata = item.metadata()?;
        if metadata.is_dir() {
            bytes += directory_bytes(&item.path())?;
        } else if metadata.is_file() {
            bytes += metadata.len();
        }
    }
    Ok(bytes)
}

fn seed(path: &Path, dataset: &str) -> Result<(), Box<dyn Error>> {
    let mut db = Database::open_with_limits(path, 2000, 500 * 1024 * 1024)?;
    let (text_count, image_count) = match dataset {
        "empty" => (0, 0),
        "text200" => (200, 0),
        "image200" => (0, 200),
        "mixed2000" => (1800, 200),
        _ => return Err("dataset must be empty, text200, image200, or mixed2000".into()),
    };
    for index in 0..text_count {
        let content = format!("测试文字 {index}: https://example.com/{index}?x=1");
        db.insert_entry(
            EntryKind::Text,
            &content,
            &format!("text-{index}"),
            None,
            None,
            None,
        )?;
    }
    for index in 0..image_count {
        let mut state = index as u32 + 1;
        let mut pixels = Vec::with_capacity(256 * 256 * 4);
        for _ in 0..(256 * 256) {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            pixels.extend_from_slice(&[(state >> 16) as u8, (state >> 8) as u8, state as u8, 255]);
        }
        db.insert_entry(
            EntryKind::Image,
            &format!("Image {index}"),
            "",
            Some(256),
            Some(256),
            Some(&pixels),
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn peak_working_set_mb(child: &std::process::Child) -> f64 {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let handle = HANDLE(child.as_raw_handle());
    if unsafe { GetProcessMemoryInfo(handle, &mut counters, counters.cb) }.is_ok() {
        counters.PeakWorkingSetSize as f64 / 1024. / 1024.
    } else {
        f64::NAN
    }
}

#[cfg(not(windows))]
fn peak_working_set_mb(_: &std::process::Child) -> f64 {
    f64::NAN
}
