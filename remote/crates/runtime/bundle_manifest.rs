pub fn entries(manifest: &serde_json::Value) -> Result<Vec<(&str, u64)>, &'static str> {
    if manifest["architecture"].as_str() != Some("x64") {
        return Err("architecture");
    }
    let files = manifest["files"].as_array().ok_or("files")?;
    if files.len() != 3 {
        return Err("file count");
    }
    let mut names = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(3);
    for file in files {
        let name = file["name"].as_str().ok_or("filename")?;
        let canonical = name.to_ascii_lowercase();
        if !matches!(
            canonical.as_str(),
            "conpty.dll" | "openconsole.exe" | "license-microsoft-conpty.txt"
        ) || !names.insert(canonical)
        {
            return Err("unexpected or duplicate filename");
        }
        let size = file["bytes"].as_u64().ok_or("size")?;
        if size == 0 || size > 8 * 1024 * 1024 {
            return Err("size");
        }
        result.push((name, size));
    }
    Ok(result)
}
