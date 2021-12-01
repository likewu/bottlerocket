use model_derive::model;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::modeled_types::Identifier;
use crate::{
    AwsSettings, BootstrapContainer, ECSSettings, HostContainer, KernelSettings, MetricsSettings,
    NetworkSettings, NtpSettings, PemCertificate, RegistrySettings, UpdatesSettings,
};

// Note: we have to use 'rename' here because the top-level Settings structure is the only one
// that uses its name in serialization; internal structures use the field name that points to it
#[model(rename = "settings", impl_default = true)]
struct Settings {
    motd: String,
    updates: UpdatesSettings,
    host_containers: HashMap<Identifier, HostContainer>,
    bootstrap_containers: HashMap<Identifier, BootstrapContainer>,
    ntp: NtpSettings,
    network: NetworkSettings,
    kernel: KernelSettings,
    aws: AwsSettings,
    ecs: ECSSettings,
    metrics: MetricsSettings,
    pki: HashMap<Identifier, PemCertificate>,
    container_registry: RegistrySettings,
}
