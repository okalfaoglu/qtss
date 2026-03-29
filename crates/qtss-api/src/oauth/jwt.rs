use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{invalid_token, OAuthErr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    /// Kullanıcı UUID (string).
    pub sub: String,
    pub org_id: String,
    pub roles: Vec<String>,
    /// Coarse capability strings derived from roles at issue time; empty in legacy tokens is backfilled in `require_jwt`.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Yetkili OAuth istemcisi (client_id).
    pub azp: String,
    pub exp: i64,
    pub iat: i64,
    pub aud: String,
    pub iss: String,
}

pub struct JwtIssuer {
    encoding: EncodingKey,
    decoding: DecodingKey,
    pub audience: String,
    pub issuer: String,
    pub access_ttl_secs: i64,
}

impl JwtIssuer {
    /// `secret` HMAC ham baytları; `secret.len()` UTF-8 bayt sayısıdır (karakter/grapheme değil).
    pub fn from_secret(
        secret: &[u8],
        audience: String,
        issuer: String,
        access_ttl_secs: i64,
    ) -> Result<Self, String> {
        if secret.len() < 32 {
            return Err("QTSS_JWT_SECRET en az 32 UTF-8 byte olmalı".into());
        }
        Ok(Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            audience,
            issuer,
            access_ttl_secs,
        })
    }

    pub fn issue_access_token(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        roles: Vec<String>,
        permissions: Vec<String>,
        client_id: &str,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let now = Utc::now().timestamp();
        let exp = now + self.access_ttl_secs;
        let claims = AccessClaims {
            sub: user_id.to_string(),
            org_id: org_id.to_string(),
            roles,
            permissions,
            azp: client_id.to_string(),
            exp,
            iat: now,
            aud: self.audience.clone(),
            iss: self.issuer.clone(),
        };
        jsonwebtoken::encode(&Header::default(), &claims, &self.encoding)
    }

    pub fn verify(&self, token: &str) -> Result<AccessClaims, OAuthErr> {
        let mut validation = Validation::default();
        validation.validate_exp = true;
        // jsonwebtoken 9: `validate_aud` açıkken `aud` claim varsa `set_audience` şart; yoksa InvalidAudience.
        validation.set_audience(&[&self.audience]);
        validation.set_issuer(&[&self.issuer]);
        let claims = jsonwebtoken::decode::<AccessClaims>(token, &self.decoding, &validation)
            .map_err(|e| invalid_token(e.to_string()))?
            .claims;
        Ok(claims)
    }
}
