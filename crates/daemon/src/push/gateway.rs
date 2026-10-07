//! Control-owned VAPID keys, subscriptions, and provider selection.

use super::{
    delivery::{PushTransport, ReqwestPushTransport},
    endpoint::{validate_push_endpoint_shape, validate_resolved_addresses},
    key_store::VapidKeyStore,
};
use crate::link_crypto;
use base64ct::{Base64UrlUnpadded, Encoding as _};
use protocol::{
    PushProviderMode, PushPublicKey, PushSubscriptionInfo, PushSubscriptionList,
    PushSubscriptionRequest, Timestamp,
};
use std::{
    future::Future,
    net::IpAddr,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
};
use store::Store;
use web_push_native::p256::PublicKey;

const MAX_DEVICES: u32 = 16;

/// Asynchronous DNS resolution injected in tests to avoid public networking.
pub type ResolveFuture = Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, String>> + Send>>;
/// Resolver function used for endpoint registration and each delivery attempt.
pub type PushResolver = Arc<dyn Fn(String, u16) -> ResolveFuture + Send + Sync>;

/// Static control-owned Push settings.
#[derive(Debug, Clone)]
pub struct PushGatewaySettings {
    /// Owner-only VAPID key file.
    pub key_file: PathBuf,
    /// Validated control HTTPS origin and VAPID subject.
    pub subject_origin: String,
    /// Daemon owner UID.
    pub owner_uid: u32,
}

/// Control-owned native Push configuration and subscription service.
#[derive(Clone)]
pub struct PushGateway {
    store: Arc<Store>,
    settings: Arc<PushGatewaySettings>,
    clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
    resolver: PushResolver,
    key: Arc<Mutex<Option<VapidKeyStore>>>,
    transport: Arc<dyn PushTransport>,
}

/// Secret-free failure suitable for a bounded control HTTP response.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PushGatewayError {
    /// The browser supplied an invalid field.
    #[error("invalid push subscription: {0}")]
    Invalid(String),
    /// Provider hostname did not resolve safely.
    #[error("push endpoint could not be accepted: {0}")]
    Endpoint(String),
    /// Owner requested a missing device.
    #[error("unknown push subscription '{0}'")]
    NotFound(String),
    /// Web Push cannot be enabled without a device.
    #[error("Web Push requires at least one active subscribed device")]
    NoDevices,
    /// Owner-only key or SQLite state failed.
    #[error("push gateway could not complete the operation")]
    Internal,
}

impl PushGateway {
    /// Create a gateway using system DNS for production delivery.
    pub fn new(
        store: Arc<Store>,
        settings: PushGatewaySettings,
        clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
    ) -> Self {
        Self::with_dependencies(
            store,
            settings,
            clock,
            system_resolver(),
            Arc::new(ReqwestPushTransport),
        )
    }

    /// Create a gateway with an injected resolver for offline tests.
    pub fn with_resolver(
        store: Arc<Store>,
        settings: PushGatewaySettings,
        clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
        resolver: PushResolver,
    ) -> Self {
        Self::with_dependencies(
            store,
            settings,
            clock,
            resolver,
            Arc::new(ReqwestPushTransport),
        )
    }

    /// Create a gateway with injected DNS and provider transport for offline tests.
    pub fn with_dependencies(
        store: Arc<Store>,
        settings: PushGatewaySettings,
        clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
        resolver: PushResolver,
        transport: Arc<dyn PushTransport>,
    ) -> Self {
        Self {
            store,
            settings: Arc::new(settings),
            clock,
            resolver,
            key: Arc::new(Mutex::new(None)),
            transport,
        }
    }

    /// Load or create the stable VAPID key after an explicit owner action.
    pub fn public_key(&self) -> Result<PushPublicKey, PushGatewayError> {
        Ok(self.key()?.public())
    }

    /// Return provider mode and secret-free owner device metadata.
    pub fn subscriptions(&self) -> Result<PushSubscriptionList, PushGatewayError> {
        Ok(PushSubscriptionList {
            provider_mode: self
                .store
                .push_provider_mode()
                .map_err(|_| PushGatewayError::Internal)?,
            devices: self
                .store
                .push_subscription_infos()
                .map_err(|_| PushGatewayError::Internal)?,
        })
    }

    /// Validate and idempotently add or refresh one browser-managed subscription.
    pub async fn subscribe(
        &self,
        request: PushSubscriptionRequest,
    ) -> Result<PushSubscriptionInfo, PushGatewayError> {
        request
            .validate()
            .map_err(|error| PushGatewayError::Invalid(error.to_string()))?;
        let public = Base64UrlUnpadded::decode_vec(&request.keys.p256dh)
            .map_err(|_| PushGatewayError::Invalid("keys.p256dh is invalid".into()))?;
        PublicKey::from_sec1_bytes(&public).map_err(|_| {
            PushGatewayError::Invalid("keys.p256dh is not a P-256 public key".into())
        })?;
        let auth = Base64UrlUnpadded::decode_vec(&request.keys.auth)
            .map_err(|_| PushGatewayError::Invalid("keys.auth is invalid".into()))?;
        if auth.len() != 16 {
            return Err(PushGatewayError::Invalid(
                "keys.auth must contain 16 bytes".into(),
            ));
        }
        let now = (self.clock)();
        if request
            .expiration_time
            .is_some_and(|expiration| expiration.0 <= now.0)
        {
            return Err(PushGatewayError::Invalid(
                "expiration_time must be in the future".into(),
            ));
        }
        let endpoint = validate_push_endpoint_shape(&request.endpoint)
            .map_err(|error| PushGatewayError::Endpoint(error.to_string()))?;
        let addresses = (self.resolver)(endpoint.host, endpoint.port)
            .await
            .map_err(|_| PushGatewayError::Endpoint("DNS resolution failed".into()))?;
        validate_resolved_addresses(&addresses)
            .map_err(|error| PushGatewayError::Endpoint(error.to_string()))?;
        let proposed_id = link_crypto::new_handle("ps").map_err(|_| PushGatewayError::Internal)?;
        let endpoint_hash = link_crypto::sha256_hex(&request.endpoint);
        self.store
            .upsert_push_subscription(&proposed_id, &endpoint_hash, &request, now, MAX_DEVICES)
            .map(|stored| stored.info)
            .map_err(|error| match error {
                store::StoreError::PushDeviceLimit { .. } => {
                    PushGatewayError::Invalid("owner already has 16 active devices".into())
                }
                _ => PushGatewayError::Internal,
            })
    }

    /// Remove exactly one owner-selected browser subscription.
    pub fn unsubscribe(&self, subscription_id: &str) -> Result<(), PushGatewayError> {
        validate_subscription_id(subscription_id)?;
        match self
            .store
            .remove_push_subscription(subscription_id)
            .map_err(|_| PushGatewayError::Internal)?
        {
            true => {
                if self
                    .store
                    .active_push_subscriptions()
                    .map_err(|_| PushGatewayError::Internal)?
                    .is_empty()
                {
                    self.store
                        .set_push_provider_mode(PushProviderMode::Disabled, (self.clock)())
                        .map_err(|_| PushGatewayError::Internal)?;
                }
                Ok(())
            }
            false => Err(PushGatewayError::NotFound(subscription_id.into())),
        }
    }

    /// Select exactly one provider; Web Push requires an active device.
    pub fn set_provider_mode(
        &self,
        mode: PushProviderMode,
    ) -> Result<PushSubscriptionList, PushGatewayError> {
        if mode == PushProviderMode::WebPush
            && self
                .store
                .active_push_subscriptions()
                .map_err(|_| PushGatewayError::Internal)?
                .is_empty()
        {
            return Err(PushGatewayError::NoDevices);
        }
        self.store
            .set_push_provider_mode(mode, (self.clock)())
            .map_err(|_| PushGatewayError::Internal)?;
        self.subscriptions()
    }

    /// Whether heartbeats should instruct workers to upload native intents.
    pub fn notification_ingress(&self) -> bool {
        self.store.push_provider_mode().ok() == Some(PushProviderMode::WebPush)
    }

    /// Current mutually exclusive provider mode, defaulting safely to disabled.
    pub fn provider_mode(&self) -> PushProviderMode {
        self.store
            .push_provider_mode()
            .unwrap_or(PushProviderMode::Disabled)
    }

    /// Validated signing key used only inside the control provider worker.
    pub(crate) fn signing_key(&self) -> Result<VapidKeyStore, PushGatewayError> {
        self.key()
    }

    /// Resolver reused immediately before a provider request.
    pub(crate) fn resolver(&self) -> &PushResolver {
        &self.resolver
    }

    /// Shared store used by the delivery scheduler.
    pub(crate) fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// Injected clock used by retry state.
    pub(crate) fn now(&self) -> Timestamp {
        (self.clock)()
    }

    pub(crate) fn transport(&self) -> &Arc<dyn PushTransport> {
        &self.transport
    }

    fn key(&self) -> Result<VapidKeyStore, PushGatewayError> {
        let mut key = self.key.lock().map_err(|_| PushGatewayError::Internal)?;
        if let Some(key) = key.as_ref() {
            return Ok(key.clone());
        }
        let loaded = VapidKeyStore::load_or_create(
            &self.settings.key_file,
            &self.settings.subject_origin,
            self.settings.owner_uid,
            (self.clock)(),
        )
        .map_err(|error| {
            tracing::error!(%error, "VAPID key unavailable");
            PushGatewayError::Internal
        })?;
        *key = Some(loaded.clone());
        Ok(loaded)
    }
}

fn system_resolver() -> PushResolver {
    Arc::new(|host, port| {
        Box::pin(async move {
            tokio::net::lookup_host((host.as_str(), port))
                .await
                .map(|addresses| addresses.map(|address| address.ip()).collect())
                .map_err(|error| error.to_string())
        })
    })
}

fn validate_subscription_id(value: &str) -> Result<(), PushGatewayError> {
    if value.len() == 19
        && value.starts_with("ps_")
        && value[3..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(PushGatewayError::Invalid(
            "subscription_id has an invalid shape".into(),
        ))
    }
}
