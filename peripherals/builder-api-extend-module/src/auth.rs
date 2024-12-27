use axum::{
    body::Body,
    extract::Request, middleware::Next, response::{Html, Response}
};
use http_body_util::BodyExt;
use crate::error::BuilderApiError;
use serde::Deserialize;
use ethers::types::{Address, Signature};


#[derive(Deserialize, Clone, Debug)]
struct AuthRequest {
    message: String,
    signature: String,
    pubkey: String
}

pub async fn auth_handler(
    req: Request,
    next: Next,
) -> Result<Response, BuilderApiError> {
    // parsing body without consuming; err: just using a work around
    let (parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|_err| BuilderApiError::InvalidParams("Missing or bad params".to_string()))?
        .to_bytes();

    let params: AuthRequest = serde_json::from_slice(&bytes.clone())
        .or_else(|_err| Err(BuilderApiError::InvalidParams("Missing or bad params".to_string())))?;

    // verification
    let message = params.message;
    let address: Address = params.pubkey.parse().or_else(|_err| Err(BuilderApiError::InvalidParams("Bad pubkey".to_string())))?;
    let signature_hash = params.signature;
    // using this for metamask signs
    let message_hash = ethers::utils::keccak256(
        format!("\x19Ethereum Signed Message:\n{}{}", message.len(), message).as_bytes()
    );
    let signature_bytes = hex::decode(&signature_hash[2..]).or_else(|_err| Err(BuilderApiError::InvalidParams("Bad signature".to_string())))?;
    let signature = Signature::try_from(signature_bytes.as_slice()).or_else(|_err| Err(BuilderApiError::InvalidParams("Bad signature".to_string())))?;

    // Recover the signer's address from the signature
    let recovered_address = signature.recover(message_hash).or_else(|_err| Err(BuilderApiError::InvalidParams("Pubkey recovery failed".to_string())))?;

    // Verify the recovered address matches the expected address
    if recovered_address == address {
        tracing::info!("Signature Verified Sucessfully");
        Ok(next.run(Request::from_parts(parts, Body::from(bytes))).await)
    } else {
        tracing::info!("Signature Verification Failed");
        Err(BuilderApiError::SignatureVerificationFailed("Invalid signature".to_string()))
    }
}

pub async fn auth_response() -> Html<& 'static str> {
    Html("Authenticated Sucessfully")
}
