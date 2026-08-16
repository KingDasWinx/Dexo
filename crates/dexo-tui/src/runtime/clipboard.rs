pub fn copy_text(text: String) -> Result<(), String> {
    copy_with_adapter(text, os_adapter)
}

fn os_adapter(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())
}

pub fn copy_with_adapter<F>(text: String, adapter: F) -> Result<(), String>
where
    F: FnOnce(String) -> Result<(), String>,
{
    adapter(text)
}

#[cfg(test)]
mod tests {
    use super::copy_with_adapter;

    #[test]
    fn headless_adapter_failure_is_err() {
        let err = copy_with_adapter("secret".into(), |_| Err("denied".into()));
        assert_eq!(err.unwrap_err(), "denied");
    }

    #[test]
    fn headless_adapter_success_is_ok() {
        assert!(copy_with_adapter("ok".into(), |_| Ok(())).is_ok());
    }
}
