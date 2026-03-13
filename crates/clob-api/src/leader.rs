use anyhow::{Context, Result};
use k8s_openapi::api::coordination::v1::Lease;
use kube::api::PostParams;
use kube::{
    api::{Api, Patch, PatchParams},
    Client,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

const LEASE_NAME: &str = "clob-leader";
const LEASE_DURATION_SECS: i32 = 15;
const RENEW_INTERVAL_SECS: u64 = 5;
const RETRY_INTERVAL_SECS: u64 = 3;

pub struct LeaderElector {
    pub is_leader: Arc<AtomicBool>,
    namespace: String,
    identity: String,
    health: crate::health::SharedHealth,
}

impl LeaderElector {
    pub fn new(
        namespace: impl Into<String>,
        identity: impl Into<String>,
        health: crate::health::SharedHealth,
    ) -> Self {
        Self {
            is_leader: Arc::new(AtomicBool::new(false)),
            namespace: namespace.into(),
            identity: identity.into(),
            health,
        }
    }

    /// Spawns background task that continuously attempts to acquire/renew the lease.
    /// Updates `is_leader` atomically on transitions.
    pub async fn run(self: Arc<Self>) -> Result<()> {
        let client = Client::try_default()
            .await
            .context("failed to create kube client")?;

        let leases: Api<Lease> = Api::namespaced(client.clone(), &self.namespace);

        loop {
            match self.try_acquire_or_renew(&leases).await {
                Ok(true) => {
                    if !self.is_leader.load(Ordering::Acquire) {
                        info!(identity = %self.identity, "acquired leader lease");
                        self.is_leader.store(true, Ordering::Release);
                        self.health.lease_healthy.store(true, Ordering::Release);
                        if let Err(e) = self.patch_pod_label("leader").await {
                            warn!("failed to patch pod label to leader: {}", e);
                        }
                    }
                    sleep(Duration::from_secs(RENEW_INTERVAL_SECS)).await;
                }
                Ok(false) => {
                    if self.is_leader.load(Ordering::Acquire) {
                        warn!(identity = %self.identity, "lost leader lease");
                        self.is_leader.store(false, Ordering::Release);
                        self.health.lease_healthy.store(false, Ordering::Release);
                        if let Err(e) = self.patch_pod_label("follower").await {
                            warn!("failed to patch pod label to follower: {}", e);
                        }
                    }
                    sleep(Duration::from_secs(RETRY_INTERVAL_SECS)).await;
                }
                Err(e) => {
                    warn!("lease error: {}", e);
                    self.is_leader.store(false, Ordering::Release);
                    sleep(Duration::from_secs(RETRY_INTERVAL_SECS)).await;
                }
            }
        }
    }

    async fn try_acquire_or_renew(&self, leases: &Api<Lease>) -> Result<bool> {
        let now = chrono::Utc::now();
        let now_k8s = k8s_openapi::apimachinery::pkg::apis::meta::v1::MicroTime(now);

        // Read current lease
        match leases.get_opt(LEASE_NAME).await? {
            None => {
                // Create fresh lease — we become leader
                let lease = Lease {
                    metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                        name: Some(LEASE_NAME.to_string()),
                        namespace: Some(self.namespace.clone()),
                        ..Default::default()
                    },
                    spec: Some(k8s_openapi::api::coordination::v1::LeaseSpec {
                        holder_identity: Some(self.identity.clone()),
                        lease_duration_seconds: Some(LEASE_DURATION_SECS),
                        acquire_time: Some(now_k8s.clone()),
                        renew_time: Some(now_k8s),
                        lease_transitions: Some(0),
                        ..Default::default()
                    }),
                };
                leases
                    .create(&Default::default(), &lease)
                    .await
                    .context("create lease")?;
                Ok(true)
            }
            Some(existing) => {
                let spec = existing.spec.as_ref();
                let holder = spec
                    .and_then(|s| s.holder_identity.as_deref())
                    .unwrap_or("");
                let renew_time = spec.and_then(|s| s.renew_time.as_ref()).map(|t| t.0);
                let duration = spec
                    .and_then(|s| s.lease_duration_seconds)
                    .unwrap_or(LEASE_DURATION_SECS);

                let expired = renew_time
                    .map(|t| now.signed_duration_since(t).num_seconds() > duration as i64)
                    .unwrap_or(true);

                let we_hold = holder == self.identity;

                if we_hold || expired {
                    // Patch to take/renew
                    let transitions = existing
                        .spec
                        .as_ref()
                        .and_then(|s| s.lease_transitions)
                        .unwrap_or(0)
                        + if we_hold { 0 } else { 1 };

                    let patch = serde_json::json!({
                        "spec": {
                            "holderIdentity": self.identity,
                            "leaseDurationSeconds": LEASE_DURATION_SECS,
                            "renewTime": now_k8s,
                            "leaseTransitions": transitions,
                        }
                    });
                    leases
                        .patch(
                            LEASE_NAME,
                            &PatchParams::apply("clob-api"),
                            &Patch::Merge(&patch),
                        )
                        .await
                        .context("patch lease")?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    pub async fn patch_pod_label(&self, role: &str) -> Result<()> {
        let client = Client::try_default()
            .await
            .context("kube client for label patch")?;

        let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, &self.namespace);

        let patch = serde_json::json!({
            "metadata": {
                "labels": {
                    "role": role
                }
            }
        });

        pods.patch(
            &self.identity,
            &PatchParams::apply("clob-api"),
            &Patch::Merge(&patch),
        )
        .await
        .context("patch pod label")?;

        info!(identity = %self.identity, role, "pod label patched");
        Ok(())
    }
}
