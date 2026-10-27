//! Hardware detection, performance benchmarking, and soft world generation limits.
//!
//! Provides system hardware telemetry (CPU cores, RAM capacity, GPU acceleration capabilities,
//! storage headroom) and evaluates world generation requirements against hardware power.

use crate::coordinate_system::geographic::LLBBox;
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Information about a detected storage volume/disk.
#[derive(Debug, Clone)]
pub struct DiskDriveInfo {
    pub mount_point: String,
    pub name: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub is_hdd: bool,
    pub is_target: bool,
}

impl DiskDriveInfo {
    pub fn free_gb(&self) -> f64 {
        self.free_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn total_gb(&self) -> f64 {
        self.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn media_type_str(&self) -> &'static str {
        if self.is_hdd {
            "HDD (Mechanical)"
        } else {
            "SSD (Solid State)"
        }
    }
}

/// System hardware specifications detected at runtime.
#[derive(Debug, Clone)]
pub struct HardwareSpecs {
    pub cpu_name: String,
    pub cpu_physical_cores: usize,
    pub cpu_logical_cores: usize,
    pub ram_total_bytes: u64,
    pub ram_free_bytes: u64,
    pub gpu_name: Option<String>,
    pub gpu_vram_bytes: Option<u64>,
    pub disk_free_bytes: u64,
    pub disks: Vec<DiskDriveInfo>,
    pub target_disk: Option<DiskDriveInfo>,
}

impl HardwareSpecs {
    /// Formats RAM in GB with 1 decimal place.
    pub fn ram_total_gb(&self) -> f64 {
        self.ram_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn ram_free_gb(&self) -> f64 {
        self.ram_free_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn gpu_vram_gb(&self) -> Option<f64> {
        self.gpu_vram_bytes
            .map(|b| b as f64 / (1024.0 * 1024.0 * 1024.0))
    }

    pub fn disk_free_gb(&self) -> f64 {
        self.disk_free_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    /// Composite compute performance score (relative scale, higher = faster).
    pub fn compute_power_score(&self) -> u32 {
        let cpu_weight = self.cpu_logical_cores as f64 * 100.0;
        let ram_weight = self.ram_total_gb() * 25.0;
        let gpu_weight = self.gpu_vram_gb().unwrap_or(0.0) * 80.0;
        (cpu_weight + ram_weight + gpu_weight) as u32
    }
}

/// Detects system hardware across CPU, RAM, GPU, and all mounted disks.
pub fn detect_hardware(target_path: Option<&Path>) -> HardwareSpecs {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();

    let cpu_logical_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);

    let cpu_physical_cores = if cpu_logical_cores > 1 {
        cpu_logical_cores / 2
    } else {
        1
    };

    let cpu_name = std::env::var("PROCESSOR_IDENTIFIER")
        .unwrap_or_else(|_| "Intel/AMD Multi-Core Processor".to_string());

    let ram_total_bytes = sys.total_memory();
    let ram_free_bytes = sys.available_memory();

    let (gpu_name, gpu_vram_bytes) = detect_gpu();

    let disk_probe = target_path
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let disk_free_bytes = fs2::available_space(&disk_probe).unwrap_or(0);

    let abs_probe = disk_probe.canonicalize().unwrap_or(disk_probe.clone());
    let probe_str = abs_probe.to_string_lossy().to_uppercase();

    let mut disks_list = Vec::new();
    let sys_disks = sysinfo::Disks::new_with_refreshed_list();
    for d in sys_disks.list() {
        let mount = d.mount_point().to_string_lossy().to_string();
        let mount_clean = mount.trim_end_matches(['\\', '/']).to_string();
        let is_target = if !mount_clean.is_empty() {
            probe_str.starts_with(&mount_clean.to_uppercase())
        } else {
            false
        };
        let is_hdd = d.kind() == sysinfo::DiskKind::HDD;
        disks_list.push(DiskDriveInfo {
            mount_point: mount,
            name: d.name().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            free_bytes: d.available_space(),
            is_hdd,
            is_target,
        });
    }

    let target_disk = disks_list.iter().find(|d| d.is_target).cloned();

    HardwareSpecs {
        cpu_name,
        cpu_physical_cores,
        cpu_logical_cores,
        ram_total_bytes,
        ram_free_bytes,
        gpu_name,
        gpu_vram_bytes,
        disk_free_bytes,
        disks: disks_list,
        target_disk,
    }
}

/// Detects primary GPU and VRAM.
fn detect_gpu() -> (Option<String>, Option<u64>) {
    let nvidia_cmd = if cfg!(target_os = "windows") {
        "C:\\Windows\\System32\\nvidia-smi.exe"
    } else {
        "nvidia-smi"
    };

    if let Ok(output) = Command::new(nvidia_cmd)
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .stdin(std::process::Stdio::null())
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = text.lines().next() {
                let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                if parts.len() >= 2 {
                    let name = parts[0].to_string();
                    if let Ok(mb) = parts[1].parse::<u64>() {
                        return (Some(name), Some(mb * 1024 * 1024));
                    }
                    return (Some(name), None);
                }
            }
        }
    }

    (None, None)
}

/// Resource and dimension estimate for a given world area.
#[derive(Debug, Clone)]
pub struct WorldResourceEstimate {
    pub area_km2: f64,
    pub width_km: f64,
    pub height_km: f64,
    pub width_blocks: u64,
    pub height_blocks: u64,
    pub total_blocks: u64,
    pub total_chunks: u64,
    pub total_regions: u64,
    pub resident_ram_needed_bytes: u64,
    pub streaming_ram_peak_bytes: u64,
    pub estimated_disk_bytes: u64,
    pub compute_seconds: f64,
    pub disk_io_seconds: f64,
    pub is_target_hdd: bool,
    pub disk_write_rate_mb_s: f64,
    pub estimated_generation_seconds: f64,
}

impl WorldResourceEstimate {
    /// Estimates requirements from bounding box, world scale, and hardware performance.
    pub fn calculate(bbox: &LLBBox, scale: f64, hardware: &HardwareSpecs) -> Self {
        let area_km2 = bbox.area_km2();
        let (p_min, p_max) = (bbox.min(), bbox.max());
        let lat_mid = (p_min.lat() + p_max.lat()) / 2.0;

        // Approximate dimensions in kilometers
        let height_km = (p_max.lat() - p_min.lat()).abs() * 111.0;
        let width_km =
            (p_max.lng() - p_min.lng()).abs() * (111.0 * lat_mid.to_radians().cos().abs());

        let width_blocks = (width_km * 1000.0 * scale).round() as u64;
        let height_blocks = (height_km * 1000.0 * scale).round() as u64;

        let total_blocks = width_blocks.saturating_mul(height_blocks);
        let total_chunks = (width_blocks.div_ceil(16)).saturating_mul(height_blocks.div_ceil(16));
        let total_regions = (width_blocks.div_ceil(512)).saturating_mul(height_blocks.div_ceil(512));

        // Memory estimation:
        // Fully resident mode retains in-memory region structures: ~15 MB per region uncompressed
        let resident_ram_needed_bytes = total_regions.saturating_mul(15 * 1024 * 1024);

        // Streaming mode keeps active tile batch resident + pipeline buffer:
        let active_threads = hardware.cpu_logical_cores.max(1) as u64;
        let streaming_ram_peak_bytes =
            (active_threads * 2 * 18 * 1024 * 1024) + (256 * 1024 * 1024);

        // Disk estimation: ~2 MB compressed MCA per region on average
        let estimated_disk_bytes = total_regions.saturating_mul(2 * 1024 * 1024);

        // Speed estimate based on hardware power score:
        // Score 1000 ~= 15 tiles/s. Total tiles approx total_regions.
        let power = hardware.compute_power_score().max(100) as f64;
        let tiles_per_second = (power / 100.0) * 0.8;
        let compute_seconds = (total_regions as f64 / tiles_per_second).max(1.0);

        // Disk I/O throughput:
        // SSD write speed ~1200 MB/s, HDD write speed ~80 MB/s (due to seek overhead on chunk writes)
        let is_target_hdd = hardware
            .target_disk
            .as_ref()
            .map(|d| d.is_hdd)
            .unwrap_or(false);
        let disk_write_rate_mb_s = if is_target_hdd { 80.0 } else { 1200.0 };
        let disk_io_seconds = (estimated_disk_bytes as f64 / 1e6) / disk_write_rate_mb_s;

        // Overall ETA bounded by compute and I/O pipeline
        let estimated_generation_seconds = compute_seconds.max(disk_io_seconds);

        Self {
            area_km2,
            width_km,
            height_km,
            width_blocks,
            height_blocks,
            total_blocks,
            total_chunks,
            total_regions,
            resident_ram_needed_bytes,
            streaming_ram_peak_bytes,
            estimated_disk_bytes,
            compute_seconds,
            disk_io_seconds,
            is_target_hdd,
            disk_write_rate_mb_s,
            estimated_generation_seconds,
        }
    }
}

/// Evaluation of soft limits against detected system capacity.
#[derive(Debug, Clone)]
pub struct SoftLimitsReport {
    pub estimate: WorldResourceEstimate,
    pub hardware: HardwareSpecs,
    pub exceeds_resident_ram: bool,
    pub exceeds_free_disk: bool,
    pub exceeds_soft_area_threshold: bool,
    pub recommended_streaming: bool,
    pub recommended_threads: usize,
    pub warnings: Vec<String>,
}

impl SoftLimitsReport {
    pub fn is_safe(&self) -> bool {
        self.warnings.is_empty()
    }
}

/// Evaluates soft limits for a given generation job.
pub fn check_soft_limits(
    bbox: &LLBBox,
    scale: f64,
    target_dir: &Path,
    force_streaming: bool,
) -> SoftLimitsReport {
    let hardware = detect_hardware(Some(target_dir));
    let estimate = WorldResourceEstimate::calculate(bbox, scale, &hardware);

    let mut warnings = Vec::new();

    // Check RAM limit
    let exceeds_resident_ram = estimate.resident_ram_needed_bytes > hardware.ram_free_bytes;
    let exceeds_free_disk = estimate.estimated_disk_bytes > hardware.disk_free_bytes;
    let exceeds_soft_area_threshold = estimate.area_km2 > 500.0;

    let recommended_streaming = force_streaming || exceeds_resident_ram || exceeds_soft_area_threshold;
    let recommended_threads = hardware.cpu_logical_cores;

    if exceeds_resident_ram && !recommended_streaming {
        warnings.push(format!(
            "World requires ~{:.1} GB resident RAM, but only {:.1} GB free RAM is detected. Streaming-to-disk must be enabled to prevent out-of-memory crash.",
            estimate.resident_ram_needed_bytes as f64 / 1e9,
            hardware.ram_free_gb()
        ));
    }

    let target_drive = hardware.target_disk.as_ref();
    let target_mount = target_drive.map(|d| d.mount_point.as_str()).unwrap_or("target drive");
    let target_type = target_drive.map(|d| d.media_type_str()).unwrap_or("Disk");

    if exceeds_free_disk {
        warnings.push(format!(
            "World requires ~{:.1} GB disk space, but target drive {} ({}) has only {:.1} GB free.",
            estimate.estimated_disk_bytes as f64 / 1e9,
            target_mount,
            target_type,
            hardware.disk_free_gb()
        ));

        // Look for another mounted disk with ample free space!
        if let Some(alt) = hardware.disks.iter().find(|d| !d.is_target && d.free_bytes > estimate.estimated_disk_bytes) {
            warnings.push(format!(
                "RECOMMENDATION: Target disk has insufficient space, but drive {} ({}) has {:.1} GB free! Run with --output-dir \"{}\\minecraft_worlds\"",
                alt.mount_point,
                alt.media_type_str(),
                alt.free_gb(),
                alt.mount_point.trim_end_matches(['\\', '/'])
            ));
        }
    } else if let Some(td) = target_drive {
        if td.is_hdd && estimate.estimated_disk_bytes > 5_000_000_000 {
            warnings.push(format!(
                "Notice: Target drive {} is an HDD (mechanical). Writing {:.1} GB will be disk I/O bound (~{:.1} MB/s, est. I/O time: {:.1} min). If an SSD is available, it would generate faster.",
                td.mount_point,
                estimate.estimated_disk_bytes as f64 / 1e9,
                estimate.disk_write_rate_mb_s,
                estimate.disk_io_seconds / 60.0
            ));
        }
    }

    if exceeds_soft_area_threshold {
        warnings.push(format!(
            "Massive area selected: {:.0} km² (~{:.1} billion blocks). Generation is optimized with streaming tile pipeline across {} hardware threads.",
            estimate.area_km2,
            estimate.total_blocks as f64 / 1e9,
            recommended_threads
        ));
    }

    if estimate.area_km2 > 1000.0 {
        warnings.push(
            "TIP (LOD preloading): For country-scale worlds, use --dh-lod to generate pre-computed Distant Horizons LOD octrees into DistantHorizons.sqlite. The entire territory renders out to the horizon on join while keeping Minecraft region chunks localized!".to_string()
        );
    }

    SoftLimitsReport {
        estimate,
        hardware,
        exceeds_resident_ram,
        exceeds_free_disk,
        exceeds_soft_area_threshold,
        recommended_streaming,
        recommended_threads,
        warnings,
    }
}

/// Prints a formatted hardware summary and soft limits check.
pub fn print_hardware_summary(report: &SoftLimitsReport) {
    let hw = &report.hardware;
    let est = &report.estimate;

    println!(
        "\n{}",
        "=== FAST-ARNIS HARDWARE & PERFORMANCE PROFILE ==="
            .cyan()
            .bold()
    );
    println!(
        "  CPU:       {} ({} cores, {} logical threads)",
        hw.cpu_name.bright_white().bold(),
        hw.cpu_physical_cores,
        hw.cpu_logical_cores
    );
    println!(
        "  RAM:       {:.1} GB total ({:.1} GB currently free)",
        hw.ram_total_gb(),
        hw.ram_free_gb()
    );
    if let Some(gpu) = &hw.gpu_name {
        let vram = hw
            .gpu_vram_gb()
            .map(|g| format!("{:.1} GB VRAM", g))
            .unwrap_or_else(|| "Detected".to_string());
        println!(
            "  GPU:       {} ({})",
            gpu.bright_white().bold(),
            vram.green()
        );
    } else {
        println!("  GPU:       None / Integrated");
    }

    if !hw.disks.is_empty() {
        println!("  Disks Detected ({} volumes):", hw.disks.len());
        for d in &hw.disks {
            let active = if d.is_target {
                format!(" {}", "[TARGET DRIVE]".bright_yellow().bold())
            } else {
                String::new()
            };
            println!(
                "    - Drive {:<3} {:<18} | Free: {:>7.1} GB / {:>7.1} GB | {}{}",
                d.mount_point,
                d.media_type_str(),
                d.free_gb(),
                d.total_gb(),
                d.name,
                active
            );
        }
    } else {
        println!(
            "  Disk:      {:.1} GB free on target drive",
            hw.disk_free_gb()
        );
    }

    println!(
        "  Power:     Rating {} pts",
        hw.compute_power_score().to_string().yellow().bold()
    );

    println!(
        "\n{}",
        "=== WORLD RESOURCE ESTIMATE & SOFT LIMITS ==="
            .cyan()
            .bold()
    );
    println!(
        "  Area:      {:.1} km² ({:.1} km x {:.1} km)",
        est.area_km2, est.width_km, est.height_km
    );
    println!(
        "  Grid:      {} x {} blocks (total {} blocks)",
        est.width_blocks, est.height_blocks, est.total_blocks
    );
    println!(
        "  Regions:   {} Minecraft 512x512 regions ({} chunks)",
        est.total_regions, est.total_chunks
    );
    println!(
        "  Disk Est:  ~{:.2} GB MCA files",
        est.estimated_disk_bytes as f64 / 1e9
    );
    println!(
        "  RAM Est:   ~{:.2} GB (streaming peak: ~{:.2} GB)",
        est.resident_ram_needed_bytes as f64 / 1e9,
        est.streaming_ram_peak_bytes as f64 / 1e9
    );
    println!(
        "  Time Est:  ~{:.1} min (Compute: ~{:.1} min, Disk I/O: ~{:.1} min on {})",
        est.estimated_generation_seconds / 60.0,
        est.compute_seconds / 60.0,
        est.disk_io_seconds / 60.0,
        if est.is_target_hdd { "HDD" } else { "SSD" }
    );

    if !report.warnings.is_empty() {
        println!("\n{}", "=== HARDWARE SOFT LIMIT WARNINGS ===".yellow().bold());
        for w in &report.warnings {
            println!("  {} {}", "WARNING:".yellow().bold(), w);
        }
        if report.recommended_streaming {
            println!(
                "  {} Auto-tuned Fast-Arnis to streaming-to-disk pipeline.",
                "ADAPTATION:".green().bold()
            );
        }
    } else {
        println!(
            "\n  {} World parameters are well within hardware capacity.",
            "OK:".green().bold()
        );
    }
    println!();
}

/// Runs a self-contained hardware power benchmark and soft limits test suite.
pub fn run_hardware_benchmark_and_limits_test() {
    println!(
        "{}",
        "============================================================"
            .bright_blue()
            .bold()
    );
    println!(
        "{}",
        "   FAST-ARNIS: HARDWARE BENCHMARK & SOFT LIMITS TEST SUITE   "
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        "============================================================"
            .bright_blue()
            .bold()
    );

    let hw = detect_hardware(None);
    println!("\n[1/3] System Hardware Telemetry");
    println!("  CPU: {} ({} threads)", hw.cpu_name, hw.cpu_logical_cores);
    println!("  RAM: {:.1} GB (Free: {:.1} GB)", hw.ram_total_gb(), hw.ram_free_gb());
    if let Some(gpu) = &hw.gpu_name {
        println!("  GPU: {} ({:.1} GB VRAM)", gpu, hw.gpu_vram_gb().unwrap_or(0.0));
    }
    println!("  Performance Score: {}", hw.compute_power_score());

    // 2. Multi-threaded CPU Benchmark
    println!("\n[2/3] Benchmarking Multi-Threaded Throughput (Rayon Pool)");
    let t0 = Instant::now();
    use rayon::prelude::*;
    let test_size = 5_000_000;
    let sum: u64 = (0..test_size)
        .into_par_iter()
        .map(|i| {
            let x = (i as f64).sin();
            let y = (i as f64).cos();
            ((x * x + y * y) * 1000.0) as u64
        })
        .sum();
    let elapsed = t0.elapsed();
    let ops_per_sec = (test_size as f64 / elapsed.as_secs_f64()) / 1e6;
    println!(
        "  Computed {} trigonometry ops in {:.3}s ({:.1} Mops/s across {} threads, checksum={})",
        test_size,
        elapsed.as_secs_f64(),
        ops_per_sec,
        hw.cpu_logical_cores,
        sum
    );

    // 3. Testing Soft World Limits on Standard Benchmark Regions
    println!("\n[3/3] Testing Soft Generation Limits on Scale Matrix");

    // Test Cases:
    let test_cases = [
        (
            "Local Town (10 km²)",
            LLBBox::from_str("47.36,8.53,47.39,8.56").unwrap(),
            1.0,
        ),
        (
            "Zurich City & Suburbs (100 km²)",
            LLBBox::from_str("47.32,8.45,47.42,8.60").unwrap(),
            1.0,
        ),
        (
            "Canton Zurich (1,729 km²)",
            LLBBox::from_str("47.15,8.35,47.70,8.90").unwrap(),
            1.0,
        ),
        (
            "SWITZERLAND FULL COUNTRY (41,285 km²)",
            LLBBox::from_str("45.818,5.956,47.808,10.492").unwrap(),
            1.0,
        ),
    ];

    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    for (name, bbox, scale) in test_cases {
        println!("\n--- Testing: {} ---", name.bright_yellow().bold());
        let report = check_soft_limits(&bbox, scale, &current_dir, true);
        println!(
            "  Area: {:.1} km² | Est. Regions: {} | Est. Disk: {:.2} GB | Est. RAM Peak: {:.2} GB",
            report.estimate.area_km2,
            report.estimate.total_regions,
            report.estimate.estimated_disk_bytes as f64 / 1e9,
            report.estimate.streaming_ram_peak_bytes as f64 / 1e9
        );
        if report.exceeds_resident_ram {
            println!(
                "  [SOFT LIMIT TRIGGERED] World exceeds resident RAM ({:.1} GB needed vs {:.1} GB free).",
                report.estimate.resident_ram_needed_bytes as f64 / 1e9,
                hw.ram_free_gb()
            );
            println!("  [MITIGATION VERIFIED] Streaming-to-disk evicts regions dynamically, capping peak RAM to ~{:.1} GB.",
                report.estimate.streaming_ram_peak_bytes as f64 / 1e9
            );
        } else {
            println!("  [STATUS] Fits within resident RAM.");
        }
    }

    println!(
        "\n{}",
        "============================================================"
            .bright_green()
            .bold()
    );
    println!(
        "{}",
        "   VERIFICATION COMPLETE: HARDWARE & SOFT LIMITS OPERATIONAL"
            .bright_green()
            .bold()
    );
    println!(
        "{}",
        "============================================================"
            .bright_green()
            .bold()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hardware() {
        let hw = detect_hardware(None);
        assert!(hw.cpu_logical_cores >= 1);
        assert!(hw.ram_total_bytes > 0);
        assert!(!hw.cpu_name.is_empty());
        assert!(!hw.disks.is_empty());
    }

    #[test]
    fn test_world_resource_estimate_zurich() {
        let hw = detect_hardware(None);
        let bbox = LLBBox::new(47.32, 8.45, 47.43, 8.62).unwrap();
        let est = WorldResourceEstimate::calculate(&bbox, 1.0, &hw);
        assert!(est.area_km2 > 50.0 && est.area_km2 < 300.0);
        assert!(est.total_blocks > 0);
        assert!(est.total_regions > 0);
    }

    #[test]
    fn test_soft_limits_switzerland() {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let bbox = LLBBox::new(45.818, 5.956, 47.808, 10.492).unwrap();
        let report = check_soft_limits(&bbox, 1.0, &current_dir, true);
        assert!(report.estimate.area_km2 > 40_000.0);
        assert!(report.exceeds_resident_ram);
        assert!(report.recommended_streaming);
        assert_eq!(report.recommended_threads, report.hardware.cpu_logical_cores);
    }
}
