
use crate::privacy::public_only_resolver;
use std::io::Read;
use std::time::Duration;

const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;

pub fn download(id: u64) -> Result<Vec<u8>, String> {
    let api = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
    let v: serde_json::Value = api
        .get(&format!("https://assetdelivery.roblox.com/v2/assetId/{}", id))
        .call()
        .map_err(|e| format!("asset {}: {}", id, e))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let url = v["locations"][0]["location"].as_str().ok_or_else(|| format!("asset {} has no download location", id))?;
    let host = url.strip_prefix("https://").and_then(|r| r.split('/').next()).unwrap_or_default();
    if !host.ends_with(".rbxcdn.com") {
        return Err("unexpected asset host".to_string());
    }
    let cdn = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).resolver(public_only_resolver).build();
    let mut out = Vec::new();
    cdn.get(url)
        .call()
        .map_err(|e| format!("asset {}: {}", id, e))?
        .into_reader()
        .take(MAX_ASSET_BYTES)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}


pub fn asset_id(url: &str) -> Option<u64> {
    let digits = |s: &str| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>();
    if let Some(r) = url.strip_prefix("rbxassetid://") {
        return digits(r).parse().ok();
    }
    let i = url.find("id=")?;
    digits(&url[i + 3..]).parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avatar::rbxm;

    #[test]
    #[ignore]
    fn dump_hat() {
        let id: u64 = std::env::var("ASSET").ok().and_then(|s| s.parse().ok()).unwrap_or(1080951);
        let bytes = download(id).unwrap();
        eprintln!("{} bytes, head {:?}", bytes.len(), &bytes[..16.min(bytes.len())]);
        let m = rbxm::parse(&bytes).unwrap();
        for (i, inst) in m.instances.iter().enumerate() {
            eprintln!("{} {} parent={:?} props={:?}", i, inst.class, inst.parent, inst.props);
        }
    }
}
