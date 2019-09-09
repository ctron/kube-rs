use std::path::Path;
#[cfg(not(feature="rustls-tls"))]
use openssl::{
    pkcs12::Pkcs12,
    pkey::PKey,
    x509::X509,
};
#[cfg(not(feature="rustls-tls"))]
use failure::ResultExt;
use crate::{Result, Error, ErrorKind};
use crate::config::apis::{AuthInfo, Cluster, Config, Context};
#[cfg(not(feature="rustls-tls"))]
use reqwest::Identity;
use reqwest::Certificate;
#[cfg(feature="rustls-tls")]
use rustls::internal::msgs::codec::Codec;

/// KubeConfigLoader loads current context, cluster, and authentication information.
#[derive(Debug)]
pub struct KubeConfigLoader {
    pub current_context: Context,
    pub cluster: Cluster,
    pub user: AuthInfo,
}

impl KubeConfigLoader {
    pub fn load<P: AsRef<Path>>(
        path: P,
        context: Option<String>,
        cluster: Option<String>,
        user: Option<String>,
    ) -> Result<KubeConfigLoader> {
        let config = Config::load_config(path)?;
        let context_name = context.as_ref().unwrap_or(&config.current_context);
        let current_context = config
            .contexts
            .iter()
            .find(|named_context| &named_context.name == context_name)
            .map(|named_context| &named_context.context)
            .ok_or_else(|| ErrorKind::KubeConfig("Unable to load current context".into()))?;
        let cluster_name = cluster.as_ref().unwrap_or(&current_context.cluster);
        let cluster = config
            .clusters
            .iter()
            .find(|named_cluster| &named_cluster.name == cluster_name)
            .map(|named_cluster| &named_cluster.cluster)
            .ok_or_else(|| ErrorKind::KubeConfig("Unable to load cluster of context".into()))?;
        let user_name = user.as_ref().unwrap_or(&current_context.user);
        let user = config
            .auth_infos
            .iter()
            .find(|named_user| &named_user.name == user_name)
            .map(|named_user| {
                let mut user = named_user.auth_info.clone();
                match user.load_gcp() {
                    Ok(_) => Ok(user),
                    Err(e) => Err(e),
                }
            })
            .ok_or_else(|| ErrorKind::KubeConfig("Unable to load user of context".into()))??;
        Ok(KubeConfigLoader {
            current_context: current_context.clone(),
            cluster: cluster.clone(),
            user: user.clone(),
        })
    }

    #[cfg(not(feature="rustls-tls"))]
    pub fn identity(&self) -> Result<reqwest::Identity> {
        let client_cert = &self.user.load_client_certificate()?;
        let client_key = &self.user.load_client_key()?;

        let x509 = X509::from_pem(&client_cert).context(ErrorKind::SslError)?;
        let pkey = PKey::private_key_from_pem(&client_key).context(ErrorKind::SslError)?;

        let p12 = Pkcs12::builder()
            .build(" ", "kubeconfig", &pkey, &x509)
            .context(ErrorKind::SslError)?;

        Ok(Identity::from_pkcs12_der(&p12.to_der().context(ErrorKind::SslError)?, " ").context(ErrorKind::SslError)?)
    }

    #[cfg(feature="rustls-tls")]
    pub fn identity(&self) -> Result<reqwest::Identity> {
        let client_cert = &self.user.load_client_certificate()?;
        let client_key = &self.user.load_client_key()?;

        let mut buffer = client_key.clone();
        buffer.extend(client_cert);

        reqwest::Identity::from_pem(buffer.as_slice()).map_err(|_|Error::from(ErrorKind::SslError))
    }

    #[cfg(not(feature="rustls-tls"))]
    pub fn ca_bundle(&self) -> Option<Result<Vec<Certificate>>> {
        let bundle = self.cluster.load_certificate_authority().ok()?;

        let bundle = X509::stack_from_pem(&bundle).map_err(|_| Error::from(ErrorKind::SslError)).ok()?;

        let mut certs = Vec::new();

        for cert in bundle {
            certs.push(Certificate::from_der(&cert.to_der().context(ErrorKind::SslError).ok()?)
                .context(ErrorKind::SslError).ok()?)
        }

        Some(Ok(certs))
    }

    #[cfg(feature="rustls-tls")]
    pub fn ca_bundle(&self) -> Option<Result<Vec<Certificate>>> {
        let bundle = self.cluster.load_certificate_authority().ok()?;

        let mut c = std::io::Cursor::new(bundle);
        let bundle = rustls::internal::pemfile::certs(&mut c).ok()?;

        let mut certs = Vec::new();

        for cert in bundle {
            certs.push(reqwest::Certificate::from_der(cert.get_encoding().as_slice()).ok()?);
        }

        Some(Ok(certs))
    }
}
