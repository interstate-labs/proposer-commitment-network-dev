use crate::error::BuilderApiError;
use axum::{
    body::Body,
    extract::Request,
    middleware::Next,
    response::Response,
    Json,
};
use ethers::types::{Address, Signature};
use http_body_util::BodyExt;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
// signature verification auth

#[derive(Deserialize, Clone, Debug)]
pub struct AuthRequest {
    message: String,
    signature: String,
    pubkey: String,
}

pub async fn signature_auth_handler(req: Request, next: Next) -> Result<Response, BuilderApiError> {
    // parsing body without consuming; err: just using a work around
    let (parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|_err| BuilderApiError::InvalidParams("Missing or bad params".to_string()))?
        .to_bytes();

    let params: AuthRequest = serde_json::from_slice(&bytes.clone()).or_else(|_err| {
        Err(BuilderApiError::InvalidParams(
            "Missing or bad params".to_string(),
        ))
    })?;

    // verification
    let message = params.message;
    let address: Address = params
        .pubkey
        .parse()
        .or_else(|_err| Err(BuilderApiError::InvalidParams("Bad pubkey".to_string())))?;
    let signature_hash = params.signature;
    // using this for metamask signs
    let message_hash = ethers::utils::keccak256(
        format!("\x19Ethereum Signed Message:\n{}{}", message.len(), message).as_bytes(),
    );
    let signature_bytes = hex::decode(&signature_hash[2..])
        .or_else(|_err| Err(BuilderApiError::InvalidParams("Bad signature".to_string())))?;
    let signature = Signature::try_from(signature_bytes.as_slice())
        .or_else(|_err| Err(BuilderApiError::InvalidParams("Bad signature".to_string())))?;

    // Recover the signer's address from the signature
    let recovered_address = signature.recover(message_hash).or_else(|_err| {
        Err(BuilderApiError::InvalidParams(
            "Pubkey recovery failed".to_string(),
        ))
    })?;

    // Verify the recovered address matches the expected address
    if recovered_address == address {
        tracing::info!("Signature Verified Sucessfully");
        Ok(next
            .run(Request::from_parts(parts, Body::from(bytes)))
            .await)
    } else {
        tracing::info!("Signature Verification Failed");
        Err(BuilderApiError::SignatureVerificationFailed(
            "Invalid signature".to_string(),
        ))
    }
}

// jwt verification auth

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Client {
    exp: usize,
    iat: usize,
    pubkey: String,
}

pub async fn jwt_auth_handler(mut req: Request, next: Next) -> Result<Response, BuilderApiError> {
    let auth_header = req.headers_mut().get(http::header::AUTHORIZATION);
    let auth_header = match auth_header {
        Some(header) => header
            .to_str()
            .map_err(|_err| BuilderApiError::JWTError("Missing JWT Header".to_string()))?,
        None => return Err(BuilderApiError::JWTError("Missing JWT Header".to_string())),
    };

    let mut header = auth_header.split_whitespace();
    let (_bearer, token) = (header.next(), header.next());
    let token_data = match decode_jwt(token.unwrap().to_string()) {
        Ok(data) => data,
        Err(_) => return Err(BuilderApiError::JWTError("Invalid Token".to_string())),
    };
    req.extensions_mut().insert(token_data.claims);
    Ok(next.run(req).await)
}

pub fn encode_jwt(pubkey: String) -> Result<String, BuilderApiError> {
    let expiry = dotenv::var("JWT_EXPIRY_HOURS").or_else(|_err| {
        Err(BuilderApiError::ENVError(
            "Failed to load env variables".to_string(),
        ))
    })?;
    let expiry: i64 = expiry.parse().or_else(|_err| {
        Err(BuilderApiError::ENVError(
            "Failed to load env variables".to_string(),
        ))
    })?;
    let secret = dotenv::var("JWT_SECRET").or_else(|_err| {
        Err(BuilderApiError::ENVError(
            "Failed to load env variables".to_string(),
        ))
    })?;

    let claim = Client {
        pubkey,
        iat: (chrono::Utc::now().timestamp()) as usize,
        exp: (chrono::Utc::now() + chrono::Duration::hours(expiry)).timestamp() as usize,
    };

    encode(
        &Header::default(),
        &claim,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .or_else(|_err| {
        Err(BuilderApiError::ENVError(
            "Failed to generate JWT".to_string(),
        ))
    })
}

pub fn decode_jwt(token: String) -> Result<jsonwebtoken::TokenData<Client>, BuilderApiError> {
    let secret = dotenv::var("JWT_SECRET").or_else(|_err| {
        Err(BuilderApiError::ENVError(
            "Failed to load env variables".to_string(),
        ))
    })?;

    decode(
        &token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    )
    .or_else(|_err| {
        Err(BuilderApiError::JWTError(
            "Failed to decode JWT".to_string(),
        ))
    })
}

pub async fn auth_response(
    Json(authbody): Json<AuthRequest>,
) -> Result<Json<String>, BuilderApiError> {
    Ok(Json(encode_jwt(authbody.pubkey)?))
}

pub async fn whitelisted_route() -> Json<String> {
    Json("success".to_string())
}
