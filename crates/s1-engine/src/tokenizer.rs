use std::path::Path;

use serde_json::Value;
use tokenizers::Tokenizer;

use crate::error::EngineError;

/// The checkpoint's Hugging Face tokenizer plus the special tokens Laya builds sequences with.
pub struct LayaTokenizer {
    tokenizer: Tokenizer,
    pub cls_id: u32,
    pub sep_id: u32,
    pub mask_id: u32,
    pub pad_id: u32,
    pub mask_token: String,
}

impl LayaTokenizer {
    pub fn load(tokenizer_path: &Path, config_path: &Path) -> Result<Self, EngineError> {
        let mut tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|error| EngineError::Tokenizer(error.to_string()))?;
        tokenizer.with_truncation(None).map_err(|error| EngineError::Tokenizer(error.to_string()))?;
        tokenizer.with_padding(None);
        let config_text = std::fs::read_to_string(config_path)
            .map_err(|source| EngineError::Io { path: config_path.to_path_buf(), source })?;
        let config: Value = serde_json::from_str(&config_text)
            .map_err(|source| EngineError::InvalidJson { path: config_path.to_path_buf(), source })?;
        let special = |field: &str, fallback: &str| -> Result<(String, u32), EngineError> {
            let token = config.get(field).and_then(Value::as_str).unwrap_or(fallback).to_string();
            let id = tokenizer.token_to_id(&token).ok_or_else(|| EngineError::MissingSpecialToken(token.clone()))?;
            Ok((token, id))
        };
        let (_, cls_id) = special("cls_token", "[CLS]")?;
        let (_, sep_id) = special("sep_token", "[SEP]")?;
        let (mask_token, mask_id) = special("mask_token", "[MASK]")?;
        let (_, pad_id) = special("pad_token", "[PAD]")?;
        Ok(Self { tokenizer, cls_id, sep_id, mask_id, pad_id, mask_token })
    }

    /// Token ids without special tokens, like `tokenizer(text, add_special_tokens=False)`.
    pub fn encode(&self, text: &str) -> Result<Vec<u32>, EngineError> {
        let encoding = self.tokenizer.encode(text, false).map_err(|error| EngineError::Tokenizer(error.to_string()))?;
        Ok(encoding.get_ids().to_vec())
    }

    /// Replaces the mask token so user text can never create a decision marker.
    pub fn neutralize(&self, text: &str) -> String {
        text.replace(&self.mask_token, " ")
    }
}
