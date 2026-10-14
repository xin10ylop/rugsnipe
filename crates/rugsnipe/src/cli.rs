//! Minimal argument helpers (no clap: the binaries take a handful of flags).

/// Value of `--name value` (last occurrence).
pub fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .rposition(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

/// Every value of a repeatable `--name value`.
pub fn opts<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| *a == name)
        .filter_map(|(i, _)| args.get(i + 1).map(|s| s.as_str()))
        .collect()
}

/// `true` if `--name` is present.
pub fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// Parsed `--name value` or `default`.
pub fn num<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match opt(args, name) {
        None => Ok(default),
        Some(v) => v
            .replace('_', "")
            .parse()
            .map_err(|_| format!("{name}: bad value {v:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse() {
        let a: Vec<String> = ["x", "--n", "1_000", "--f", "--s", "a", "--s", "b"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(num::<u64>(&a, "--n", 0).unwrap(), 1000);
        assert!(flag(&a, "--f") && !flag(&a, "--g"));
        assert_eq!(opt(&a, "--s"), Some("b"));
        assert_eq!(opts(&a, "--s"), vec!["a", "b"]);
        assert!(num::<u64>(&a, "--s", 0).is_err());
    }
}
