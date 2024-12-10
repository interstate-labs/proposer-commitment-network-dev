use crate::{builder::{BuilderClient, GetHeaderParams, GetPayloadResponse, SignedBuilderBid, VersionedValue}, error::BuilderApiError};
use ethereum_consensus::{
  deneb::mainnet::SignedBlindedBeaconBlock,
  builder::SignedValidatorRegistration
};
use reqwest::{ StatusCode, Url};


use futures::future::join_all;

#[derive(Clone)]
pub struct Extender {
  pub builders: Vec<BuilderClient>
}

impl Extender {
  pub fn new(urls:Vec<String>) -> Extender {
    let mut extender = Extender {
      builders: Vec::new()
    };

    for url in urls {
      extender.builders.push(BuilderClient::new(Url::parse(&url).unwrap()));
    }

    extender
  }
  
  pub async fn status(&self) -> Result<StatusCode, BuilderApiError> {
    tracing::debug!("Sending status request to multiple builders");

    let mut handles =Vec::with_capacity(self.builders.len());

    for builder in &self.builders {
      let nbuilder = builder.clone();
      handles.push(tokio::spawn(async move{
        nbuilder.status().await
      }));
    }
   
    let mut error = BuilderApiError::FailedJoinningInExtender("status".to_string());

    let results = join_all(handles).await;
    for res in results.into_iter() {
      match res {
        Ok(result) => {
          match result {
            Ok(status) => { return Ok(status)},
            Err(err) => error = err
          }
        } ,
        Err(err) => {
          tracing::debug!( ?err, "Errors in joinning handles");
        },
      }
    }
    Err(error)
  }

  pub async fn register_validators(
    &self,
    registrations: Vec<SignedValidatorRegistration>,
  ) -> Result<(), BuilderApiError> {
    tracing::debug!("Sending register_validator request to multiple builders");

    let mut handles =Vec::with_capacity(self.builders.len());

    for builder in &self.builders {
      let nbuilder = builder.clone();
      let nregistrations: Vec<SignedValidatorRegistration> = registrations.clone();
      handles.push(tokio::spawn(async move{
        nbuilder.register_validators(nregistrations).await
      }));
    }

    let mut error = BuilderApiError::FailedJoinningInExtender("status".to_string());

    let results= join_all(handles).await;

    for res in results.into_iter() {
      match res {
        Ok(result) => {
          match result {
            Ok(_) => { return Ok(())},
            Err(err) => error = err
          }
        } ,
        Err(err) => {
          tracing::debug!( ?err, "Errors in joinning handles");
        },
      }
    }
    
    Err(error)

  }

  pub async fn get_header(
    &self,
    params: GetHeaderParams,
  ) -> Result<VersionedValue<SignedBuilderBid>, BuilderApiError> {
    tracing::debug!("Sending get_header request to multiple builders");

    let mut handles =Vec::with_capacity(self.builders.len());

    for builder in &self.builders {
      let nbuilder = builder.clone();
      let params = params.clone();
      let url = &nbuilder.url;
      handles.push(tokio::spawn(async move{
        nbuilder.get_header(params).await
      }));
    }
   
    let mut error = BuilderApiError::FailedJoinningInExtender("status".to_string());

    let results = join_all(handles).await;
    for res in results.into_iter() {
      match res {
        Ok(result) => {
          match result {
            Ok(signed_bid) => { return Ok(signed_bid)},
            Err(err) => error = err
          }
        } ,
        Err(err) => {
          tracing::debug!( ?err, "Errors in joinning handles");
        },
      }
    }
    Err(error)
  }

  pub async fn get_payload(
    &self,
    signed_block: SignedBlindedBeaconBlock,
  ) -> Result<GetPayloadResponse, BuilderApiError> {
    tracing::debug!("Sending get_payload request to multiple builders");

    let mut handles =Vec::with_capacity(self.builders.len());

    for builder in &self.builders {
      let nbuilder = builder.clone();
      let signed_block = signed_block.clone();
      handles.push(tokio::spawn(async move{
        nbuilder.get_payload(signed_block).await
      }));
    }
   
    let mut error = BuilderApiError::FailedJoinningInExtender("status".to_string());

    let results = join_all(handles).await;
    for res in results.into_iter() {
      match res {
        Ok(result) => {
          match result {
            Ok(payload) => { return Ok(payload)},
            Err(err) => error = err
          }
        } ,
        Err(err) => {
          tracing::debug!( ?err, "Errors in joinning handles");
        },
      }
    }
    Err(error)
  }
  
}

#[cfg(test)]
 mod extender_tests {
  use super::*;

  #[test]
  fn test_from_urls() {
    let urls = vec![
      "http://test1.com".to_string(),
      "http://test2.com".to_string()
    ];

    let extender = Extender::new(urls);
    assert_eq!(extender.builders.len(), 2);
    assert_eq!(extender.builders[0].url.as_str(), "http://test1.com/");
    assert_eq!(extender.builders[1].url.as_str(), "http://test2.com/");
  }

  // Test with empty input
  #[test]
  fn test_from_urls_empty() {
      let urls: Vec<String> = Vec::new();

      let extender = Extender::new(urls);

      // No builders should be added
      assert_eq!(extender.builders.len(), 0);
  }

  // Test with invalid URL (this will panic due to .unwrap())
  #[test]
  #[should_panic]
  fn test_from_urls_invalid_url() {
      let urls = vec![
          "http://example.com".to_string(),
          "invalid-url".to_string(), // Invalid URL to trigger a panic
      ];

      Extender::new(urls);
  }

 }
