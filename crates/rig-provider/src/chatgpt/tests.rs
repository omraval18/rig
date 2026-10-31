use super::login::{query_param, urldecode, urlencode};
use super::*;
use sha2::{Digest, Sha256};

#[test]
fn pkce_challenge_matches_rfc7636_example() {
    // RFC 7636 appendix B.
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(
        B64.encode(Sha256::digest(verifier.as_bytes())),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn url_codec_roundtrip() {
    let s = "http://localhost:1455/auth/callback?x=a b&é";
    assert_eq!(urldecode(&urlencode(s)), s);
    assert_eq!(
        query_param("code=abc%2Fdef&state=s", "code").as_deref(),
        Some("abc/def")
    );
}

#[test]
fn account_id_prefers_stored_then_claims() {
    assert_eq!(
        account_id_from(&json!({"account_id": "acc"})).as_deref(),
        Some("acc")
    );
    let payload =
        B64.encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"from-jwt"}}"#);
    let jwt = format!("h.{payload}.s");
    assert_eq!(
        account_id_from(&json!({"access_token": jwt})).as_deref(),
        Some("from-jwt")
    );
}

#[test]
fn login_url_carries_pkce_parameters() {
    let p = LoginPlan::new().unwrap();
    for needle in [
        "code_challenge_method=S256",
        "response_type=code",
        "redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback",
        "offline_access",
    ] {
        assert!(p.url.contains(needle), "missing {needle}");
    }
}
