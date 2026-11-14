use crate::ids::UserId;
use std::fmt::Debug;

use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use jsonwebtoken::{DecodingKey, EncodingKey};

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct JWTClaims {
    sub: UserId,
    exp: u64,
}

impl JWTClaims {
    #[must_use]
    pub fn new(sub: UserId, exp: u64) -> Self {
        Self { sub, exp }
    }

    #[must_use]
    pub fn sub(&self) -> UserId {
        self.sub
    }

    #[must_use]
    pub fn exp(&self) -> u64 {
        self.exp
    }
}

/// Configuration for issuing and validating the `jwt` session cookie.
///
/// ```
/// use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Validation};
/// use einrad_server::jwt::{JWTClaims, JWTConfig};
/// use einrad_server::ids::UserId;
///
/// let secret = b"a-test-secret";
/// let config = JWTConfig::new(
///     EncodingKey::from_secret(secret),
///     DecodingKey::from_secret(secret),
///     Algorithm::HS512,
///     Validation::new(Algorithm::HS512),
///     false,
/// );
///
/// let user_id = UserId::new();
/// let token = config.create_user_token(user_id).unwrap();
/// let decoded =
///     jsonwebtoken::decode::<JWTClaims>(&token, config.decode_key(), config.validation())
///         .unwrap();
/// assert_eq!(decoded.claims.sub(), user_id);
/// ```
#[derive(Clone)]
pub struct JWTConfig {
    encoding_key: EncodingKey,
    decode_key: DecodingKey,
    algorithm: jsonwebtoken::Algorithm,
    validation: jsonwebtoken::Validation,
    validity: std::time::Duration,
    insecure_cookies: bool,
}

impl JWTConfig {
    #[must_use]
    pub fn new(
        encoding_key: EncodingKey,
        decode_key: DecodingKey,
        algorithm: jsonwebtoken::Algorithm,
        validation: jsonwebtoken::Validation,
        insecure_cookies: bool,
    ) -> Self {
        Self {
            encoding_key,
            decode_key,
            algorithm,
            validation,
            validity: std::time::Duration::from_secs(60 * 60 * 24 * 31 * 6),
            insecure_cookies,
        }
    }

    #[must_use]
    pub fn get_header(&self) -> jsonwebtoken::Header {
        jsonwebtoken::Header::new(self.algorithm)
    }

    #[must_use]
    pub fn encoding_key(&self) -> &EncodingKey {
        &self.encoding_key
    }

    #[must_use]
    pub fn decode_key(&self) -> &DecodingKey {
        &self.decode_key
    }

    #[must_use]
    pub fn validation(&self) -> &jsonwebtoken::Validation {
        &self.validation
    }

    pub fn create_user_token(
        &self,
        user_id: UserId,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let claims = JWTClaims::new(
            user_id,
            (std::time::SystemTime::now() + self.validity)
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        jsonwebtoken::encode(&self.get_header(), &claims, self.encoding_key())
    }

    pub fn add_jwt_cookie(
        &self,
        cookies: CookieJar,
        user_id: UserId,
    ) -> Result<CookieJar, jsonwebtoken::errors::Error> {
        Ok(cookies.add(
            Cookie::build(("jwt", self.create_user_token(user_id)?))
                .http_only(true)
                .secure(!self.insecure_cookies)
                .max_age(time::Duration::try_from(self.validity).unwrap())
                .path("/")
                .same_site(SameSite::Strict)
                .build(),
        ))
    }

    pub fn remove_jwt_cookie(
        &self,
        cookies: CookieJar,
    ) -> Result<CookieJar, jsonwebtoken::errors::Error> {
        Ok(cookies.add(
            Cookie::build(("jwt", ""))
                .http_only(true)
                .secure(!self.insecure_cookies)
                .max_age(time::Duration::seconds(0))
                .path("/")
                .same_site(SameSite::Strict)
                .build(),
        ))
    }
}

impl Debug for JWTConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JWTConfig")
            .field("encoding_key", &"...")
            .field("decode_key", &"...")
            .field("algorithm", &self.algorithm)
            .field("validation", &self.validation)
            .field("validity", &self.validity)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{Algorithm, Validation};

    const SECRET: &[u8] = b"unit-test-secret";

    fn config(insecure_cookies: bool) -> JWTConfig {
        JWTConfig::new(
            EncodingKey::from_secret(SECRET),
            DecodingKey::from_secret(SECRET),
            Algorithm::HS512,
            Validation::new(Algorithm::HS512),
            insecure_cookies,
        )
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn claims_accessors_return_constructor_values() {
        let id = UserId::new();
        let claims = JWTClaims::new(id, 42);
        assert_eq!(claims.sub(), id);
        assert_eq!(claims.exp(), 42);
    }

    #[test]
    fn user_token_round_trips_and_expires_in_about_six_months() {
        let config = config(false);
        let user_id = UserId::new();
        let token = config.create_user_token(user_id).unwrap();

        let data =
            jsonwebtoken::decode::<JWTClaims>(&token, config.decode_key(), config.validation())
                .unwrap();
        assert_eq!(data.header.alg, Algorithm::HS512);
        assert_eq!(data.claims.sub(), user_id);

        let expected_exp = now_secs() + 60 * 60 * 24 * 31 * 6;
        assert!(data.claims.exp().abs_diff(expected_exp) < 60);
    }

    #[test]
    fn token_signed_with_other_secret_is_rejected() {
        let config = config(false);
        let other = JWTConfig::new(
            EncodingKey::from_secret(b"other-secret"),
            DecodingKey::from_secret(b"other-secret"),
            Algorithm::HS512,
            Validation::new(Algorithm::HS512),
            false,
        );
        let token = other.create_user_token(UserId::new()).unwrap();
        assert!(
            jsonwebtoken::decode::<JWTClaims>(&token, config.decode_key(), config.validation())
                .is_err()
        );
    }

    #[test]
    fn expired_token_is_rejected() {
        let config = config(false);
        let claims = JWTClaims::new(UserId::new(), now_secs() - 3600);
        let token =
            jsonwebtoken::encode(&config.get_header(), &claims, config.encoding_key()).unwrap();
        let err =
            jsonwebtoken::decode::<JWTClaims>(&token, config.decode_key(), config.validation())
                .unwrap_err();
        assert_eq!(
            err.kind(),
            &jsonwebtoken::errors::ErrorKind::ExpiredSignature
        );
    }

    #[test]
    fn jwt_cookie_has_hardened_attributes() {
        let user_id = UserId::new();
        let jar = config(false)
            .add_jwt_cookie(CookieJar::new(), user_id)
            .unwrap();
        let cookie = jar.get("jwt").unwrap();

        assert!(!cookie.value().is_empty());
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(
            cookie.max_age(),
            Some(time::Duration::seconds(60 * 60 * 24 * 31 * 6))
        );
    }

    #[test]
    fn insecure_cookies_disables_secure_flag() {
        let jar = config(true)
            .add_jwt_cookie(CookieJar::new(), UserId::new())
            .unwrap();
        assert_eq!(jar.get("jwt").unwrap().secure(), Some(false));
    }

    #[test]
    fn remove_jwt_cookie_clears_value_and_expires_immediately() {
        let config = config(false);
        let jar = config
            .add_jwt_cookie(CookieJar::new(), UserId::new())
            .unwrap();
        let jar = config.remove_jwt_cookie(jar).unwrap();
        let cookie = jar.get("jwt").unwrap();

        assert_eq!(cookie.value(), "");
        assert_eq!(cookie.max_age(), Some(time::Duration::ZERO));
        assert_eq!(cookie.path(), Some("/"));
    }

    #[test]
    fn debug_output_does_not_leak_keys() {
        let debug = format!("{:?}", config(false));
        assert!(debug.contains("encoding_key: \"...\""));
        assert!(!debug.contains("unit-test-secret"));
    }
}
