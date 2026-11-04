//! new-api 登录密码加密。站点开启「登录密码加密」时，密码不能明文提交：
//! 生成随机 AES-256 密钥，用站点公钥（RSA-OAEP / SHA-256，label "password-v2"）包装，
//! 再用 AES-GCM 加密密码（附加数据 "password-v2:<kid>"），拼成
//! `v2.<包装后的密钥>.<nonce>.<密文>`（标准 Base64）。与 new-api `common/password_crypto.go` 对应。

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::RngCore;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Oaep, RsaPublicKey};
use sha2::Sha256;

/// 用站点公钥（SPKI PEM）加密密码，返回 v2 格式的密文。
pub fn encrypt_password(
    public_key_pem: &str,
    key_id: &str,
    password: &str,
) -> Result<String, String> {
    let public_key = RsaPublicKey::from_public_key_pem(public_key_pem.trim())
        .map_err(|error| format!("invalid public key: {error}"))?;
    let mut rng = rand::thread_rng();

    let mut aes_key = [0u8; 32];
    rng.fill_bytes(&mut aes_key);
    let wrapped = public_key
        .encrypt(
            &mut rng,
            Oaep::new_with_label::<Sha256, _>("password-v2"),
            &aes_key,
        )
        .map_err(|error| format!("wrap key: {error}"))?;

    let mut nonce = [0u8; 12];
    rng.fill_bytes(&mut nonce);
    let cipher =
        Aes256Gcm::new_from_slice(&aes_key).map_err(|error| format!("aes key: {error}"))?;
    let aad = format!("password-v2:{key_id}");
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: password.as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|error| format!("encrypt: {error}"))?;

    Ok(format!(
        "v2.{}.{}.{}",
        STANDARD.encode(wrapped),
        STANDARD.encode(nonce),
        STANDARD.encode(ciphertext)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::RsaPrivateKey;
    use rsa::pkcs8::{EncodePublicKey, LineEnding};
    use rsa::traits::PublicKeyParts;

    /// 测试时现场生成一次性密钥，仓库里不放任何私钥。
    fn test_key_pair() -> (RsaPrivateKey, String) {
        let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let public_pem = private_key
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .unwrap();
        (private_key, public_pem)
    }

    /// 按 new-api 服务端的步骤解密（common/password_crypto.go DecryptPassword 的 v2 分支）。
    fn decrypt_like_server(private_key: &RsaPrivateKey, envelope: &str, key_id: &str) -> String {
        let parts: Vec<&str> = envelope.split('.').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "v2");
        let wrapped = STANDARD.decode(parts[1]).unwrap();
        assert_eq!(
            wrapped.len(),
            private_key.size(),
            "wrapped key has the RSA modulus size"
        );
        let nonce = STANDARD.decode(parts[2]).unwrap();
        assert_eq!(nonce.len(), 12);
        let ciphertext = STANDARD.decode(parts[3]).unwrap();
        let aes_key = private_key
            .decrypt(Oaep::new_with_label::<Sha256, _>("password-v2"), &wrapped)
            .unwrap();
        let cipher = Aes256Gcm::new_from_slice(&aes_key).unwrap();
        let aad = format!("password-v2:{key_id}");
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .unwrap();
        String::from_utf8(plain).unwrap()
    }

    #[test]
    fn encrypted_password_round_trips_with_the_server_scheme() {
        let (private_key, public_pem) = test_key_pair();
        let envelope = encrypt_password(&public_pem, "kid-123", "p@ssw0rd 中文").unwrap();
        assert_eq!(
            decrypt_like_server(&private_key, &envelope, "kid-123"),
            "p@ssw0rd 中文"
        );
        // 每次加密都换新的 AES 密钥和随机数，同一个密码两次结果不同。
        assert_ne!(
            encrypt_password(&public_pem, "kid-123", "p@ssw0rd 中文").unwrap(),
            envelope
        );
    }

    #[test]
    fn rejects_invalid_public_keys() {
        assert!(encrypt_password("not a key", "kid", "secret").is_err());
    }
}
