//! Process and asset memory sampling for the Stats panel.

use eme::prelude::*;

/// Resident set size of the current process, in bytes.
pub fn parse_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())?;
    Some(kib.saturating_mul(1024))
}

/// Reads the resident set size of the current process.
pub fn process_rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_rss(&status)
}

/// Refreshes the memory rows once per second.
pub fn sample_memory(
    time: Res<Time>,
    mut stats: ResMut<crate::screens::editor::docks::EditorStats>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    mut elapsed: Local<f32>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 1.0 {
        return;
    }
    *elapsed = 0.0;
    let rss_mb = process_rss().map_or(0.0, |bytes| bytes as f32 / (1024.0 * 1024.0));
    let (meshes, materials, images) = (meshes.len(), materials.len(), images.len());
    if stats.rss_mb != rss_mb
        || stats.meshes != meshes
        || stats.materials != materials
        || stats.images != images
    {
        stats.rss_mb = rss_mb;
        stats.meshes = meshes;
        stats.materials = materials;
        stats.images = images;
        stats.rev = stats.rev.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Name:\teme_editor\nVmPeak:\t 2400000 kB\nVmRSS:\t 524288 kB\nThreads:\t12\n";

    #[test]
    fn resident_memory_is_read_in_bytes() {
        assert_eq!(parse_rss(SAMPLE), Some(524_288 * 1024));
    }

    #[test]
    fn a_status_without_rss_reports_none() {
        assert_eq!(parse_rss("Name:\teme_editor\n"), None);
        assert_eq!(parse_rss("VmRSS:\tnot-a-number kB\n"), None);
        assert_eq!(parse_rss(""), None);
    }

    #[test]
    fn the_live_process_has_resident_memory() {
        let rss = process_rss().expect("/proc/self/status is readable on Linux");
        assert!(rss > 1024 * 1024, "rss = {rss}");
    }
}
