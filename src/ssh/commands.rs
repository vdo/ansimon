use anyhow::{Context, Result};

use crate::metrics::Metrics;

/// Single remote command that collects all metrics from a Linux host.
/// Uses section markers for robust parsing. Two-sample reads (stat, net/dev,
/// diskstats) are grouped around a single `sleep 1` for delta calculation.
pub fn metrics_command() -> &'static str {
    concat!(
        "echo '===STAT1'; cat /proc/stat | head -1; ",
        "echo '===NETDEV1'; cat /proc/net/dev; ",
        "echo '===DISKSTATS1'; cat /proc/diskstats; ",
        "sleep 1; ",
        "echo '===STAT2'; cat /proc/stat | head -1; ",
        "echo '===NETDEV2'; cat /proc/net/dev; ",
        "echo '===DISKSTATS2'; cat /proc/diskstats; ",
        "echo '===MEMINFO'; cat /proc/meminfo | head -20; ",
        "echo '===DF'; df -P / | tail -1; ",
        "echo '===LOADAVG'; cat /proc/loadavg; ",
        "echo '===UPTIME'; cat /proc/uptime; ",
        "echo '===NPROC'; nproc; ",
        "echo '===SOCKSTAT'; cat /proc/net/sockstat"
    )
}

/// Parse the output of the metrics command into a Metrics struct.
/// Uses section markers for robust, order-independent parsing.
pub fn parse_metrics_output(output: &str) -> Result<Metrics> {
    let sections = parse_sections(output);

    let stat1 = sections.get("STAT1").context("Missing ===STAT1 section")?;
    let stat2 = sections.get("STAT2").context("Missing ===STAT2 section")?;
    let meminfo = sections.get("MEMINFO").context("Missing ===MEMINFO section")?;
    let df = sections.get("DF").context("Missing ===DF section")?;
    let loadavg = sections.get("LOADAVG").context("Missing ===LOADAVG section")?;
    let uptime = sections.get("UPTIME").context("Missing ===UPTIME section")?;
    let nproc = sections.get("NPROC").context("Missing ===NPROC section")?;

    // CPU + IO wait from stat delta
    let stat1_line = stat1.lines().next().unwrap_or("");
    let stat2_line = stat2.lines().next().unwrap_or("");
    let (cpu_percent, iowait_percent) =
        parse_cpu_delta(stat1_line, stat2_line).context("Failed to parse CPU")?;

    // Memory + Swap
    let meminfo_lines: Vec<&str> = meminfo.lines().collect();
    let (mem_used_gb, mem_total_gb) =
        parse_meminfo(&meminfo_lines).context("Failed to parse memory")?;
    let (swap_used_gb, swap_total_gb) =
        parse_swap(&meminfo_lines).unwrap_or((0.0, 0.0));

    // Disk usage
    let df_line = df.lines().next().unwrap_or("");
    let disk_percent = parse_df(df_line).context("Failed to parse disk")?;

    // Load average + procs
    let loadavg_line = loadavg.lines().next().unwrap_or("");
    let (load_1, load_5, load_15) =
        parse_loadavg(loadavg_line).context("Failed to parse load")?;
    let (procs_running, procs_total) =
        parse_procs(loadavg_line).unwrap_or((0, 0));

    // Uptime
    let uptime_line = uptime.lines().next().unwrap_or("");
    let uptime_secs = parse_uptime(uptime_line).unwrap_or(0);

    // Nproc
    let num_cpus = nproc
        .lines()
        .next()
        .and_then(|l| l.trim().parse::<u32>().ok())
        .unwrap_or(1);

    // Net RX/TX (delta of two samples)
    let (net_rx_bytes_sec, net_tx_bytes_sec) = match (
        sections.get("NETDEV1"),
        sections.get("NETDEV2"),
    ) {
        (Some(nd1), Some(nd2)) => parse_net_delta(nd1, nd2).unwrap_or((0, 0)),
        _ => (0, 0),
    };

    // Disk I/O (delta of two samples)
    let (disk_read_bytes_sec, disk_write_bytes_sec) = match (
        sections.get("DISKSTATS1"),
        sections.get("DISKSTATS2"),
    ) {
        (Some(ds1), Some(ds2)) => parse_diskstats_delta(ds1, ds2).unwrap_or((0, 0)),
        _ => (0, 0),
    };

    // TCP connections
    let tcp_conns = sections
        .get("SOCKSTAT")
        .and_then(|s| parse_tcp_conns(s))
        .unwrap_or(0);

    Ok(Metrics {
        cpu_percent,
        mem_used_gb,
        mem_total_gb,
        disk_percent,
        load_1,
        load_5,
        load_15,
        uptime_secs,
        num_cpus,
        iowait_percent,
        swap_used_gb,
        swap_total_gb,
        net_rx_bytes_sec,
        net_tx_bytes_sec,
        tcp_conns,
        procs_running,
        procs_total,
        disk_read_bytes_sec,
        disk_write_bytes_sec,
    })
}

/// Split output by ===SECTION markers into a map of section_name -> content.
fn parse_sections(output: &str) -> std::collections::HashMap<&str, &str> {
    let mut sections = std::collections::HashMap::new();
    let mut current_key: Option<&str> = None;
    let mut current_start: Option<usize> = None;

    for (byte_offset, line) in line_offsets(output) {
        if let Some(key) = line.strip_prefix("===") {
            // Close previous section
            if let (Some(k), Some(start)) = (current_key, current_start) {
                let end = byte_offset;
                let content = &output[start..end];
                sections.insert(k, content.trim());
            }
            current_key = Some(key);
            current_start = Some(byte_offset + line.len() + 1); // +1 for newline
        }
    }
    // Close last section
    if let (Some(k), Some(start)) = (current_key, current_start) {
        let content = &output[start.min(output.len())..];
        sections.insert(k, content.trim());
    }

    sections
}

/// Iterator over (byte_offset, line) pairs.
fn line_offsets(s: &str) -> Vec<(usize, &str)> {
    let mut result = Vec::new();
    let mut offset = 0;
    for line in s.lines() {
        result.push((offset, line));
        offset += line.len() + 1; // +1 for \n
    }
    result
}

/// Parse CPU delta from two /proc/stat "cpu" lines.
/// Returns (cpu_percent, iowait_percent).
fn parse_cpu_delta(line1: &str, line2: &str) -> Result<(f64, f64)> {
    let v1 = parse_cpu_line(line1)?;
    let v2 = parse_cpu_line(line2)?;

    let idle1 = v1.get(3).copied().unwrap_or(0);
    let idle2 = v2.get(3).copied().unwrap_or(0);
    let iowait1 = v1.get(4).copied().unwrap_or(0);
    let iowait2 = v2.get(4).copied().unwrap_or(0);

    let total1: u64 = v1.iter().sum();
    let total2: u64 = v2.iter().sum();

    let total_delta = total2.saturating_sub(total1) as f64;
    let idle_delta = (idle2 + iowait2).saturating_sub(idle1 + iowait1) as f64;
    let iowait_delta = iowait2.saturating_sub(iowait1) as f64;

    if total_delta == 0.0 {
        return Ok((0.0, 0.0));
    }

    let cpu_pct = ((total_delta - idle_delta) / total_delta * 100.0).clamp(0.0, 100.0);
    let iowait_pct = (iowait_delta / total_delta * 100.0).clamp(0.0, 100.0);

    Ok((cpu_pct, iowait_pct))
}

fn parse_cpu_line(line: &str) -> Result<Vec<u64>> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() || !parts[0].starts_with("cpu") {
        anyhow::bail!("Not a cpu line: {line}");
    }
    parts[1..]
        .iter()
        .map(|p| p.parse::<u64>().context("Failed to parse CPU value"))
        .collect()
}

fn parse_meminfo(lines: &[&str]) -> Result<(f64, f64)> {
    let mut total_kb: u64 = 0;
    let mut available_kb: u64 = 0;
    let mut free_kb: u64 = 0;
    let mut buffers_kb: u64 = 0;
    let mut cached_kb: u64 = 0;

    for line in lines {
        if let Some(val) = extract_meminfo_value(line, "MemTotal:") {
            total_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "MemAvailable:") {
            available_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "MemFree:") {
            free_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "Buffers:") {
            buffers_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "Cached:") {
            cached_kb = val;
        }
    }

    if total_kb == 0 {
        anyhow::bail!("Could not find MemTotal");
    }

    let avail = if available_kb > 0 {
        available_kb
    } else {
        free_kb + buffers_kb + cached_kb
    };

    let used_kb = total_kb.saturating_sub(avail);
    let total_gb = total_kb as f64 / 1_048_576.0;
    let used_gb = used_kb as f64 / 1_048_576.0;

    Ok((used_gb, total_gb))
}

fn parse_swap(lines: &[&str]) -> Result<(f64, f64)> {
    let mut swap_total_kb: u64 = 0;
    let mut swap_free_kb: u64 = 0;

    for line in lines {
        if let Some(val) = extract_meminfo_value(line, "SwapTotal:") {
            swap_total_kb = val;
        } else if let Some(val) = extract_meminfo_value(line, "SwapFree:") {
            swap_free_kb = val;
        }
    }

    let swap_total_gb = swap_total_kb as f64 / 1_048_576.0;
    let swap_used_gb = swap_total_kb.saturating_sub(swap_free_kb) as f64 / 1_048_576.0;

    Ok((swap_used_gb, swap_total_gb))
}

fn extract_meminfo_value(line: &str, prefix: &str) -> Option<u64> {
    if line.starts_with(prefix) {
        line[prefix.len()..]
            .split_whitespace()
            .next()
            .and_then(|v| v.parse().ok())
    } else {
        None
    }
}

fn parse_df(line: &str) -> Result<f64> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        anyhow::bail!("Unexpected df output: {line}");
    }
    let pct_str = parts[4].trim_end_matches('%');
    pct_str
        .parse::<f64>()
        .context("Failed to parse disk percentage")
}

fn parse_loadavg(line: &str) -> Result<(f64, f64, f64)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        anyhow::bail!("Unexpected loadavg: {line}");
    }
    Ok((
        parts[0].parse().context("load1")?,
        parts[1].parse().context("load5")?,
        parts[2].parse().context("load15")?,
    ))
}

/// Parse running/total processes from /proc/loadavg field 4 (e.g. "3/120").
fn parse_procs(line: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        anyhow::bail!("Unexpected loadavg for procs: {line}");
    }
    let procs_parts: Vec<&str> = parts[3].split('/').collect();
    if procs_parts.len() < 2 {
        anyhow::bail!("Unexpected procs format: {}", parts[3]);
    }
    Ok((
        procs_parts[0].parse().context("procs running")?,
        procs_parts[1].parse().context("procs total")?,
    ))
}

fn parse_uptime(line: &str) -> Result<u64> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("Empty uptime");
    }
    let secs_f: f64 = parts[0].parse().context("uptime seconds")?;
    Ok(secs_f as u64)
}

/// Parse /proc/net/dev and sum RX bytes (col 1) and TX bytes (col 9) across
/// all non-lo interfaces.
fn parse_net_dev(content: &str) -> (u64, u64) {
    let mut rx_total: u64 = 0;
    let mut tx_total: u64 = 0;

    for line in content.lines() {
        let line = line.trim();
        // Skip header lines (they contain | or don't have a colon)
        if !line.contains(':') || line.starts_with("Inter") || line.starts_with("face") {
            continue;
        }
        // Format: "iface: rx_bytes rx_packets ... tx_bytes tx_packets ..."
        if let Some((_iface, rest)) = line.split_once(':') {
            let iface = _iface.trim();
            if iface == "lo" {
                continue;
            }
            let vals: Vec<u64> = rest
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            // col 0 = rx_bytes, col 8 = tx_bytes
            if vals.len() >= 9 {
                rx_total += vals[0];
                tx_total += vals[8];
            }
        }
    }

    (rx_total, tx_total)
}

/// Compute net bytes/sec from two /proc/net/dev samples taken 1s apart.
fn parse_net_delta(content1: &str, content2: &str) -> Result<(u64, u64)> {
    let (rx1, tx1) = parse_net_dev(content1);
    let (rx2, tx2) = parse_net_dev(content2);
    Ok((rx2.saturating_sub(rx1), tx2.saturating_sub(tx1)))
}

/// Parse /proc/diskstats and sum sectors read/written for real block devices.
/// Returns (sectors_read, sectors_written).
fn parse_diskstats(content: &str) -> (u64, u64) {
    let mut reads: u64 = 0;
    let mut writes: u64 = 0;

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // /proc/diskstats has at least 14 fields
        if parts.len() < 14 {
            continue;
        }
        let dev_name = parts[2];
        // Filter to real block devices: skip partitions (ends with digit but
        // also has letters), dm-*, loop*, ram*. Include sda, vda, nvme0n1 etc.
        if dev_name.starts_with("loop")
            || dev_name.starts_with("ram")
            || dev_name.starts_with("dm-")
        {
            continue;
        }
        // Skip partitions: if the name ends with a digit and the char before
        // it is also a digit or 'p' followed by digit (like sda1, nvme0n1p1)
        if is_partition(dev_name) {
            continue;
        }
        // Field 5 (index 5) = sectors read, field 9 (index 9) = sectors written
        let sr: u64 = parts[5].parse().unwrap_or(0);
        let sw: u64 = parts[9].parse().unwrap_or(0);
        reads += sr;
        writes += sw;
    }

    (reads, writes)
}

/// Heuristic to detect partition names (e.g. sda1, nvme0n1p1).
fn is_partition(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    // If it ends with a digit
    if !bytes[bytes.len() - 1].is_ascii_digit() {
        return false;
    }
    // For nvme: contains 'p' followed by digits at end (nvme0n1p1)
    if name.starts_with("nvme") {
        // nvme0n1 is the whole disk, nvme0n1p1 is a partition
        // Check if there's a 'p' after 'n' followed by digits
        if let Some(n_pos) = name.rfind('n') {
            let after_n = &name[n_pos + 1..];
            if after_n.contains('p') {
                return true;
            }
        }
        return false;
    }
    // For sd*, vd*, hd*, xvd*: name ends with digit = partition
    // Whole disk: sda, vdb; Partition: sda1, vdb2
    if name.starts_with("sd")
        || name.starts_with("vd")
        || name.starts_with("hd")
        || name.starts_with("xvd")
    {
        return true; // ends with digit = partition (checked above)
    }
    false
}

/// Compute disk I/O bytes/sec from two /proc/diskstats samples taken 1s apart.
fn parse_diskstats_delta(content1: &str, content2: &str) -> Result<(u64, u64)> {
    let (sr1, sw1) = parse_diskstats(content1);
    let (sr2, sw2) = parse_diskstats(content2);
    // Each sector is 512 bytes
    let read_bytes_sec = sr2.saturating_sub(sr1) * 512;
    let write_bytes_sec = sw2.saturating_sub(sw1) * 512;
    Ok((read_bytes_sec, write_bytes_sec))
}

/// Parse TCP connections from /proc/net/sockstat.
/// Looks for line: "TCP: inuse N ..."
fn parse_tcp_conns(content: &str) -> Option<u32> {
    for line in content.lines() {
        if line.starts_with("TCP:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // "TCP: inuse N orphan N tw N alloc N mem N"
            if parts.len() >= 3 && parts[1] == "inuse" {
                return parts[2].parse().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sections() {
        let output = "===STAT1\ncpu 100 200 300 400\n===STAT2\ncpu 110 210 310 410\n===MEMINFO\nMemTotal: 8000000 kB\nMemAvailable: 4000000 kB\n";
        let sections = parse_sections(output);
        assert!(sections.contains_key("STAT1"));
        assert!(sections.contains_key("STAT2"));
        assert!(sections.contains_key("MEMINFO"));
        assert!(sections.get("STAT1").unwrap().starts_with("cpu"));
    }

    #[test]
    fn test_parse_cpu_delta_with_iowait() {
        let line1 = "cpu  1000 200 300 5000 100 0 0 0 0 0";
        let line2 = "cpu  1100 220 320 5050 120 0 0 0 0 0";
        let (cpu, iow) = parse_cpu_delta(line1, line2).unwrap();
        assert!(cpu > 0.0);
        assert!(iow >= 0.0);
    }

    #[test]
    fn test_parse_swap() {
        let lines = vec![
            "MemTotal:       8000000 kB",
            "SwapTotal:      2000000 kB",
            "SwapFree:       1500000 kB",
        ];
        let (used, total) = parse_swap(&lines).unwrap();
        assert!(total > 0.0);
        assert!(used > 0.0);
        assert!(used < total);
    }

    #[test]
    fn test_parse_net_dev() {
        let content = "Inter-|   Receive    |  Transmit\n face |bytes    packets  errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n    lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n  eth0: 5000 50 0 0 0 0 0 0 3000 30 0 0 0 0 0 0\n";
        let (rx, tx) = parse_net_dev(content);
        assert_eq!(rx, 5000); // eth0 only, lo excluded
        assert_eq!(tx, 3000);
    }

    #[test]
    fn test_parse_tcp_conns() {
        let content = "sockets: used 150\nTCP: inuse 42 orphan 0 tw 10 alloc 50 mem 5\nUDP: inuse 3\n";
        assert_eq!(parse_tcp_conns(content), Some(42));
    }

    #[test]
    fn test_parse_procs() {
        let line = "0.50 0.30 0.20 3/120 12345";
        let (running, total) = parse_procs(line).unwrap();
        assert_eq!(running, 3);
        assert_eq!(total, 120);
    }

    #[test]
    fn test_is_partition() {
        assert!(is_partition("sda1"));
        assert!(is_partition("vdb2"));
        assert!(is_partition("nvme0n1p1"));
        assert!(!is_partition("nvme0n1"));
        assert!(!is_partition("loop0"));
    }

    #[test]
    fn test_human_bytes() {
        use crate::metrics::human_bytes;
        assert_eq!(human_bytes(500), "500B");
        assert_eq!(human_bytes(1024), "1.0K");
        assert_eq!(human_bytes(1_048_576), "1.0M");
        assert_eq!(human_bytes(1_073_741_824), "1.0G");
    }

    #[test]
    fn test_full_marker_parse() {
        let output = "\
===STAT1
cpu  1000 200 300 5000 100 0 0 0 0 0
===NETDEV1
Inter-|   Receive
 face |bytes
    lo: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0
  eth0: 5000 50 0 0 0 0 0 0 3000 30 0 0 0 0 0 0
===DISKSTATS1
   8       0 sda 100 0 2000 0 50 0 1000 0 0 0 0 0 0 0
===STAT2
cpu  1100 220 320 5050 120 0 0 0 0 0
===NETDEV2
Inter-|   Receive
 face |bytes
    lo: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0
  eth0: 6000 60 0 0 0 0 0 0 4000 40 0 0 0 0 0 0
===DISKSTATS2
   8       0 sda 110 0 2200 0 60 0 1100 0 0 0 0 0 0 0
===MEMINFO
MemTotal:       8000000 kB
MemFree:        2000000 kB
MemAvailable:   4000000 kB
Buffers:         500000 kB
Cached:         1500000 kB
SwapTotal:      2000000 kB
SwapFree:       1500000 kB
===DF
/dev/sda1       100000 30000 70000 30% /
===LOADAVG
0.50 0.30 0.20 3/120 12345
===UPTIME
86400.50 172800.00
===NPROC
4
===SOCKSTAT
sockets: used 150
TCP: inuse 42 orphan 0 tw 10 alloc 50 mem 5
UDP: inuse 3";

        let m = parse_metrics_output(output).unwrap();
        assert!(m.cpu_percent > 0.0);
        assert!(m.iowait_percent >= 0.0);
        assert!(m.mem_total_gb > 0.0);
        assert!(m.swap_total_gb > 0.0);
        assert_eq!(m.disk_percent, 30.0);
        assert_eq!(m.load_1, 0.50);
        assert_eq!(m.num_cpus, 4);
        assert_eq!(m.tcp_conns, 42);
        assert_eq!(m.procs_running, 3);
        assert_eq!(m.procs_total, 120);
        assert_eq!(m.net_rx_bytes_sec, 1000);
        assert_eq!(m.net_tx_bytes_sec, 1000);
        assert!(m.uptime_secs == 86400);
    }
}
