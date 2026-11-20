//! 密钥库：仅存 MCP env/headers、CLI env 模板等密钥（AES-256-GCM）。
//! git 与模型凭据不经 Fux 存储（用户在节点自理）。
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};

fn key() -> [u8; 32] {
    let secret = std::env::var("FUX_MASTER_KEY").unwrap_or_else(|_| "fux-dev-master-key".into());
    let mut k = [0u8; 32];
    // 简单拉伸：重复填充 + 长度混合（生产建议提供 64 位 hex 密钥）
    let bytes = secret.as_bytes();
    for (i, b) in k.iter_mut().enumerate() {
        *b = bytes[i % bytes.len()]
            .wrapping_mul(31)
            .wrapping_add(i as u8);
    }
    k
}

pub fn encrypt(plain: &str) -> Result<String> {
    let cipher = Aes256Gcm::new_from_slice(&key()).map_err(|e| anyhow!("{e}"))?;
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plain.as_bytes())
        .map_err(|e| anyhow!("加密失败: {e}"))?;
    Ok(format!("{}{}", hex::encode(nonce_bytes), hex::encode(ct)))
}

pub fn decrypt(enc: &str) -> Result<String> {
    if enc.len() < 24 {
        return Err(anyhow!("密文格式错误"));
    }
    let nonce_bytes = hex::decode(&enc[..24])?;
    let ct = hex::decode(&enc[24..])?;
    let cipher = Aes256Gcm::new_from_slice(&key()).map_err(|e| anyhow!("{e}"))?;
    let pt = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ct.as_ref())
        .map_err(|_| anyhow!("解密失败（密钥不匹配？）"))?;
    Ok(String::from_utf8(pt)?)
}

/// CLI env 模板加密后入库（与 MCP env 同一套密钥体系）。
pub fn cli_env_encrypt(env: &Value) -> Result<String> {
    if env.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return Ok(String::new());
    }
    encrypt(&env.to_string())
}

/// CLI env 模板解密（供派发时注入子进程环境）。
/// 兼容加密上线前的明文存量数据：解密失败时按明文 JSON 解析。
pub fn cli_env_plain(env_template: &str) -> Value {
    if env_template.is_empty() {
        return json!({});
    }
    match decrypt(env_template) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(json!({})),
        Err(_) => serde_json::from_str(env_template).unwrap_or(json!({})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let enc = super::encrypt("secret-token-123").unwrap();
        assert_ne!(enc, "secret-token-123");
        assert_eq!(super::decrypt(&enc).unwrap(), "secret-token-123");
    }

    #[test]
    fn cli_env_plain_accepts_ciphertext_and_legacy_plaintext() {
        let enc = super::cli_env_encrypt(&json!({ "TOKEN": "abc" })).unwrap();
        assert_ne!(enc, "{\"TOKEN\":\"abc\"}");
        assert_eq!(super::cli_env_plain(&enc), json!({ "TOKEN": "abc" }));
        // 存量明文仍可读取
        assert_eq!(
            super::cli_env_plain("{\"TOKEN\":\"abc\"}"),
            json!({ "TOKEN": "abc" })
        );
        assert_eq!(super::cli_env_plain(""), json!({}));
        // 空模板不落密文
        assert_eq!(super::cli_env_encrypt(&json!({})).unwrap(), "");
    }
}
