/// blake3 fingerprint of some bytes as hex: notices changed files and names project indexes.
pub struct ContentFingerprint;

impl ContentFingerprint {
    pub fn of(bytes: &[u8]) -> String {
        blake3::hash(bytes).to_hex().to_string()
    }
}
