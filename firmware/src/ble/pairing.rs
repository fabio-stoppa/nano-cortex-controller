pub fn needs_pairing_recovery(details: &str) -> bool {
    details.contains("INSUFFICIENT_AUTHENTICATION")
        || details.contains("INSUFFICIENT_ENCRYPTION")
        || details.contains("InsufficientAuthentication")
        || details.contains("InsufficientEncryption")
        || details.contains("AttErrorCode { value: 5 }")
        || details.contains("AttErrorCode { value: 15 }")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_and_encryption_codes_need_pairing_recovery() {
        assert!(needs_pairing_recovery("AttErrorCode { value: 5 }"));
        assert!(needs_pairing_recovery("AttErrorCode { value: 15 }"));
        assert!(!needs_pairing_recovery("AttErrorCode { value: 3 }"));
    }
}
