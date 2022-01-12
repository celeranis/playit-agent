use std::net::SocketAddr;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use messages::{AgentRegistered, TunnelRequest};
use messages::agent_config::AgentConfig;
use messages::api::{AgentApiRequest, AgentApiResponse, ExchangeClaimForSecret, SessionSecret};
use messages::rpc::SignedRpcRequest;

pub struct ApiClient {
    api_base: String,
    agent_secret: Option<String>,
    request: Client,
}

impl ApiClient {
    pub fn new(api_base: String, agent_secret: Option<String>) -> Self {
        ApiClient {
            api_base,
            agent_secret,
            request: Client::new(),
        }
    }

    pub async fn get_control_addr(&self) -> Result<SocketAddr, ApiError> {
        match self.req(&AgentApiRequest::GetControlAddr).await? {
            AgentApiResponse::ControlAddress(addr) => Ok(addr.control_address),
            resp => Err(ApiError::UnexpectedResponse(resp)),
        }
    }

    pub async fn sign_tunnel_request(
        &self,
        request: TunnelRequest,
    ) -> Result<SignedRpcRequest<TunnelRequest>, ApiError> {
        match self
            .req(&AgentApiRequest::SignControlRequest(request))
            .await?
        {
            AgentApiResponse::SignedTunnelRequest(resp) => Ok(resp),
            resp => Err(ApiError::UnexpectedResponse(resp)),
        }
    }

    pub async fn generate_shared_tunnel_secret(
        &self,
        registered: AgentRegistered,
    ) -> Result<SessionSecret, ApiError> {
        match self
            .req(&AgentApiRequest::GenerateSharedTunnelSecret(registered))
            .await?
        {
            AgentApiResponse::SessionSecret(resp) => Ok(resp),
            resp => Err(ApiError::UnexpectedResponse(resp)),
        }
    }

    pub async fn try_exchange_claim_for_secret(&self, claim_url: &str) -> Result<Option<String>, ApiError> {
        let res = self.req(&AgentApiRequest::ExchangeClaimForSecret(ExchangeClaimForSecret {
            claim_key: claim_url.to_string()
        })).await;

        match res {
            Ok(AgentApiResponse::AgentSecret(secret)) => Ok(Some(secret.secret_key)),
            Ok(other) => Err(ApiError::UnexpectedResponse(other)),
            Err(ApiError::HttpError(404, _)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn get_agent_config(&self) -> Result<AgentConfig, ApiError> {
        let res = self.req(&AgentApiRequest::GetAgentConfig).await;

        match res {
            Ok(AgentApiResponse::AgentConfig(config)) => Ok(config),
            Ok(other) => Err(ApiError::UnexpectedResponse(other)),
            Err(error) => Err(error),
        }
    }

    async fn req(&self, req: &AgentApiRequest) -> Result<AgentApiResponse, ApiError> {
        let mut builder = self.request.post(&self.api_base);
        if let Some(secret) = &self.agent_secret {
            builder = builder.header(
                reqwest::header::AUTHORIZATION,
                format!("agent-key {}", secret),
            );
        }

        let bytes = builder.json(req).send().await?.bytes().await?;

        let result = match serde_json::from_slice::<Response>(bytes.as_ref()) {
            Ok(v) => v,
            Err(error) => {
                let content = String::from_utf8_lossy(bytes.as_ref());
                tracing::error!(?error, %content, "failed to parse response");
                return Err(ApiError::ParseError(error));
            }
        };

        match result {
            Response::Error(MiscError::Error { code, message }) => {
                Err(ApiError::HttpError(code, message))
            }
            Response::Ok(v) => Ok(v),
        }
    }
}

#[derive(Debug)]
pub enum ApiError {
    HttpError(u16, String),
    ParseError(serde_json::Error),
    RequestError(reqwest::Error),
    UnexpectedResponse(AgentApiResponse),
}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        ApiError::RequestError(error)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Response {
    Ok(AgentApiResponse),
    Error(MiscError),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum MiscError {
    #[serde(rename = "error")]
    Error { code: u16, message: String },
}