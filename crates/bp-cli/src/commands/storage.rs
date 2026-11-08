use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, CreditStat, PouchStat, StorageInfoData};
use std::io::IsTerminal;

const BAR: usize = 40;

pub async fn storage_info(network_id: String) -> anyhow::Result<()> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::StorageInfo { network_id })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let d: StorageInfoData = serde_json::from_value(data)?;

    if d.pouches.is_empty() {
        println!("💼 No Pouch here — offer space with: bp hatch pouch --network <id> --tier T1");
    }
    for p in &d.pouches {
        print_pouch(p);
        println!();
    }
    for c in &d.credits {
        print_credit(c);
        println!();
    }
    println!(
        "📤 Your files: {} ({})",
        d.files_uploaded,
        fmt_bytes(d.uploaded_bytes)
    );
    Ok(())
}

pub fn print_pouch(p: &PouchStat) {
    let free = p
        .quota_bytes
        .saturating_sub(p.fragment_bytes + p.filler_bytes);
    println!(
        "💼 Pouch on {} — {} · {} offered",
        p.network_id,
        p.tier,
        fmt_bytes(p.quota_bytes)
    );
    println!(
        "   {}",
        bar(
            p.quota_bytes,
            &[(p.fragment_bytes, GREEN), (p.filler_bytes, CYAN)]
        )
    );
    println!(
        "   {} others' data {} ({})   {} filler {} ({})   {} free {} ({})",
        paint("■", GREEN),
        fmt_bytes(p.fragment_bytes),
        pct(p.fragment_bytes, p.quota_bytes),
        paint("■", CYAN),
        fmt_bytes(p.filler_bytes),
        pct(p.filler_bytes, p.quota_bytes),
        paint("■", GREY),
        fmt_bytes(free),
        pct(free, p.quota_bytes),
    );
    println!(
        "   setup {}: the space the network has filled and challenges you on",
        pct(p.fragment_bytes + p.filler_bytes, p.quota_bytes)
    );
}

fn print_credit(c: &CreditStat) {
    let scale = c.offered_bytes.max(c.credit_bytes);
    let left = c.credit_bytes.saturating_sub(c.used_bytes);
    println!("💳 Credit on {}", c.network_id);
    println!(
        "   {}",
        bar(
            scale,
            &[(c.used_bytes.min(c.credit_bytes), YELLOW), (left, GREEN)]
        )
    );
    println!(
        "   {} used {}   {} you can still store {}   {} not verified yet {}",
        paint("■", YELLOW),
        fmt_bytes(c.used_bytes),
        paint("■", GREEN),
        fmt_bytes(left),
        paint("■", GREY),
        fmt_bytes(scale.saturating_sub(c.credit_bytes)),
    );
    println!(
        "   usable {} of the {} you offer ({}), {} of it used",
        fmt_bytes(c.credit_bytes),
        fmt_bytes(c.offered_bytes),
        pct(c.credit_bytes, c.offered_bytes),
        pct(c.used_bytes, c.credit_bytes),
    );
}

const GREEN: &str = "32";
const CYAN: &str = "36";
const YELLOW: &str = "33";
const GREY: &str = "90";

fn colors() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn paint(text: &str, color: &str) -> String {
    if colors() {
        format!("\x1b[{color}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn bar(total: u64, segments: &[(u64, &str)]) -> String {
    let glyphs = ["█", "▓"];
    let mut out = String::new();
    let mut used = 0;
    for (i, (bytes, color)) in segments.iter().enumerate() {
        let cells = if total == 0 {
            0
        } else {
            ((*bytes as f64 / total as f64) * BAR as f64).round() as usize
        }
        .min(BAR - used);
        out.push_str(&paint(&glyphs[i % 2].repeat(cells), color));
        used += cells;
    }
    out.push_str(&paint(&"░".repeat(BAR - used), GREY));
    format!("[{out}]")
}

fn pct(part: u64, whole: u64) -> String {
    if whole == 0 {
        "0%".into()
    } else {
        format!("{:.0}%", part as f64 / whole as f64 * 100.0)
    }
}

pub fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GiB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.1} MiB", b as f64 / (1u64 << 20) as f64)
    } else if b >= 1 << 10 {
        format!("{:.1} KiB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}
