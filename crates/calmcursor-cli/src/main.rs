//! `calmcursor-cli`: run Calmcursor without the desktop app, inspect the
//! system, and render the "before and after" chart from a simulated tremor.

mod chart;

use calmcursor_core::calibration::{self, CalibrationRecording};
use calmcursor_core::simulate::{Demo, TremorModel};
use calmcursor_core::version::BuildInfo;
use calmcursor_core::Settings;
use calmcursor_platform::{storage, PermissionStatus, Steadier};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{mpsc, LazyLock};
use std::time::Duration;

static LONG_VERSION: LazyLock<String> = LazyLock::new(long_version);

#[derive(Parser)]
#[command(
    name = "calmcursor-cli",
    version = calmcursor_core::version::VERSION,
    long_version = LONG_VERSION.as_str(),
    about = "Steadies the mouse pointer for people with hand tremor.",
    after_help = "The desktop app (Calmcursor) offers the same steadying with a \
                  calibration game and live trace. Both share one settings file."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Steady the pointer until Ctrl+C is pressed.
    Run {
        /// Override steadiness, 0 (light) to 100 (strongest).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        steadiness: Option<u8>,
        /// Turn off click lock (holding the pointer still while clicking).
        #[arg(long)]
        no_click_lock: bool,
        /// Use a simulated shaky hand instead of the real mouse.
        #[arg(long)]
        simulated: bool,
        /// Print live statistics every second.
        #[arg(long, short)]
        verbose: bool,
    },
    /// Show permissions, backend, version and the settings file.
    Status,
    /// Print the current settings as JSON.
    Settings,
    /// Simulate a shaky hand through the real filter and draw an SVG chart.
    Simulate {
        /// mild, moderate or strong.
        #[arg(long, default_value = "moderate")]
        tremor: String,
        /// Steadiness, 0 (light) to 100 (strongest).
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u8).range(0..=100))]
        steadiness: u8,
        /// Seconds to simulate.
        #[arg(long, default_value_t = 6.0)]
        seconds: f64,
        /// Write the chart here (otherwise print a summary only).
        #[arg(long)]
        svg: Option<PathBuf>,
        /// Random seed, for reproducible output.
        #[arg(long, default_value_t = 11)]
        seed: u64,
    },
    /// Analyse a calibration recording (JSON exported from the app).
    Analyze {
        /// Path to the recording.
        recording: PathBuf,
    },
}

fn long_version() -> String {
    let b = BuildInfo::current();
    format!(
        "{}\nbuild:   {}\ncommit:  {}\ndate:    {}\nchannel: {}\ntarget:  {}\nbackend: {}",
        b.version,
        b.build_number,
        b.git_sha,
        b.build_date,
        b.channel,
        b.target,
        calmcursor_platform::BACKEND
    )
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Run {
            steadiness,
            no_click_lock,
            simulated,
            verbose,
        } => run(steadiness, no_click_lock, simulated, verbose),
        Command::Status => status(),
        Command::Settings => {
            println!(
                "{}",
                serde_json::to_string_pretty(&storage::load()).unwrap_or_default()
            );
            Ok(())
        }
        Command::Simulate {
            tremor,
            steadiness,
            seconds,
            svg,
            seed,
        } => simulate(&tremor, steadiness, seconds, svg, seed),
        Command::Analyze { recording } => analyze(&recording),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    steadiness: Option<u8>,
    no_click_lock: bool,
    simulated: bool,
    verbose: bool,
) -> Result<(), String> {
    let mut settings = storage::load();
    settings.enabled = true;
    if let Some(s) = steadiness {
        settings.filter.steadiness = f64::from(s);
        settings.filter.custom = None;
    }
    if no_click_lock {
        settings.click.anchor_enabled = false;
    }
    let steadier = if simulated {
        Steadier::start_simulated(&settings, TremorModel::moderate())
    } else {
        Steadier::start(&settings).map_err(|e| match e {
            calmcursor_platform::PlatformError::PermissionDenied { message, help } => {
                format!("{message}\n\n{help}")
            }
            calmcursor_platform::PlatformError::NoDevices { help } => {
                format!("no mouse or trackball found. {help}")
            }
            other => other.to_string(),
        })?
    };
    let p = settings.filter.params();
    println!(
        "Calmcursor {} is steadying the pointer ({}).",
        calmcursor_core::version::VERSION,
        steadier.backend_name()
    );
    println!(
        "Filter: min cutoff {:.2} Hz, beta {:.4}. Click lock: {}. Press Ctrl+C to stop.",
        p.min_cutoff,
        p.beta,
        if settings.click.anchor_enabled {
            "on"
        } else {
            "off"
        }
    );

    let (tx, rx) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })
    .map_err(|e| format!("cannot install Ctrl+C handler: {e}"))?;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let st = steadier.status();
                if let Some(w) = &st.warning {
                    log::warn!("{w}");
                }
                if verbose {
                    println!(
                        "moves {:>7}  clicks {:>4}  steadied {:>4}  bounces dropped {:>3}  devices: {}",
                        st.stats.motion_events,
                        st.stats.clicks,
                        st.stats.steadied_clicks,
                        st.stats.suppressed_presses,
                        if st.devices.is_empty() { "-".into() } else { st.devices.join(", ") }
                    );
                }
            }
        }
    }
    steadier.stop();
    println!("Stopped. The pointer is back to normal.");
    Ok(())
}

fn status() -> Result<(), String> {
    println!("Calmcursor {}", BuildInfo::current().long_version());
    println!("Backend:     {}", calmcursor_platform::BACKEND);
    let perm = match calmcursor_platform::permission_status() {
        PermissionStatus::Granted => "granted".to_string(),
        PermissionStatus::Unknown => "unknown (try `calmcursor-cli run`)".to_string(),
        PermissionStatus::Needed { message, help } => format!("NEEDED: {message}\n\n{help}"),
    };
    println!("Permission:  {perm}");
    match storage::settings_path() {
        Some(p) => println!(
            "Settings:    {}{}",
            p.display(),
            if p.exists() {
                ""
            } else {
                " (not created yet; defaults in use)"
            }
        ),
        None => println!("Settings:    (no config directory)"),
    }
    Ok(())
}

fn simulate(
    tremor: &str,
    steadiness: u8,
    seconds: f64,
    svg: Option<PathBuf>,
    seed: u64,
) -> Result<(), String> {
    let model = TremorModel::by_name(tremor)
        .ok_or_else(|| format!("unknown tremor '{tremor}' (use mild, moderate or strong)"))?;
    let mut settings = Settings::default();
    settings.filter.steadiness = f64::from(steadiness);
    // A small area, like moving between nearby buttons; it also makes the
    // shake easy to see in the chart.
    let (w, h) = (330.0, 205.0);
    let mut demo = Demo::new(&settings, model, w, h, seed);
    demo.advance(1.0);
    demo.drain_trace();
    let mut trace = Vec::new();
    let steps = (seconds.clamp(1.0, 60.0) * 100.0) as usize;
    for _ in 0..steps {
        demo.advance(0.01);
        trace.extend(demo.drain_trace());
    }
    let summary = chart::Summary::from_trace(&trace, settings.tick_hz as f64);
    println!(
        "Simulated {:.1} s of a {} tremor ({:.1} Hz, about ±{:.0} px) at steadiness {steadiness}.",
        seconds, tremor, model.frequency_hz, model.amplitude_px
    );
    println!(
        "Shake at rest: {:.2} px raw -> {:.2} px steadied ({:.0}% less). Clicks: {}.",
        summary.raw_jitter,
        summary.smooth_jitter,
        summary.reduction_percent(),
        demo.engine().stats().clicks
    );
    if let Some(path) = svg {
        let doc = chart::render(&trace, &summary, model, steadiness, w, h);
        std::fs::write(&path, doc).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        println!("Chart written to {}", path.display());
    }
    Ok(())
}

fn analyze(path: &PathBuf) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let rec: CalibrationRecording =
        serde_json::from_str(&text).map_err(|e| format!("not a calibration recording: {e}"))?;
    let report = calibration::analyze(&rec, 0).map_err(|e| e.to_string())?;
    let mut value = serde_json::to_value(&report).map_err(|e| e.to_string())?;
    // The replays are long; leave them out of the printed summary.
    if let Some(obj) = value.as_object_mut() {
        obj.remove("hold_replay");
        obj.remove("follow_replay");
        if let Some(p) = obj.get_mut("profile").and_then(|p| p.as_object_mut()) {
            p.remove("spectrum");
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_default()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn long_version_lists_build_details() {
        let v = long_version();
        assert!(v.contains("build:") && v.contains("commit:"));
    }
}
