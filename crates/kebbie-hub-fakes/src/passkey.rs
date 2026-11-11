use webauthn_authenticator_rs::AuthenticatorBackend;
use webauthn_authenticator_rs::prelude::{
    PublicKeyCredential, RegisterPublicKeyCredential, Url, WebauthnCError,
};
use webauthn_authenticator_rs::softpasskey::SoftPasskey;
use webauthn_rs_proto::{
    AllowCredentials, PublicKeyCredentialCreationOptions, PublicKeyCredentialRequestOptions,
};

// The software passkey keeps no credential on itself, so this one remembers which user each resident credential belongs to and answers a sign-in without a list the way a phone does.
pub struct DiscoverablePasskey {
    inner: SoftPasskey,
    keeps_resident: bool,
    resident: Vec<(Vec<u8>, Vec<u8>)>,
}

impl DiscoverablePasskey {
    pub fn new(falsify_uv: bool) -> DiscoverablePasskey {
        DiscoverablePasskey {
            inner: SoftPasskey::new(falsify_uv),
            keeps_resident: true,
            resident: Vec::new(),
        }
    }

    pub fn without_resident_keys(falsify_uv: bool) -> DiscoverablePasskey {
        DiscoverablePasskey {
            keeps_resident: false,
            ..DiscoverablePasskey::new(falsify_uv)
        }
    }

    pub fn resident_count(&self) -> usize {
        self.resident.len()
    }
}

impl AuthenticatorBackend for DiscoverablePasskey {
    fn perform_register(
        &mut self,
        origin: Url,
        mut options: PublicKeyCredentialCreationOptions,
        timeout_ms: u32,
    ) -> Result<RegisterPublicKeyCredential, WebauthnCError> {
        let wants_resident = options
            .authenticator_selection
            .as_mut()
            .is_some_and(|selection| std::mem::take(&mut selection.require_resident_key));
        let user = options.user.id.as_ref().to_vec();
        let registered = AuthenticatorBackend::perform_register(&mut self.inner, origin, options, timeout_ms)?;
        if wants_resident && self.keeps_resident {
            self.resident
                .push((registered.raw_id.as_ref().to_vec(), user));
        }
        Ok(registered)
    }

    fn perform_auth(
        &mut self,
        origin: Url,
        mut options: PublicKeyCredentialRequestOptions,
        timeout_ms: u32,
    ) -> Result<PublicKeyCredential, WebauthnCError> {
        if options.allow_credentials.is_empty() {
            if self.resident.is_empty() {
                return Err(WebauthnCError::NoSelectedToken);
            }
            options.allow_credentials = self
                .resident
                .iter()
                .map(|(id, _)| AllowCredentials {
                    type_: "public-key".to_string(),
                    id: id.clone().into(),
                    transports: None,
                })
                .collect();
        }
        let mut credential = AuthenticatorBackend::perform_auth(&mut self.inner, origin, options, timeout_ms)?;
        let used = credential.raw_id.as_ref().to_vec();
        if let Some((_, user)) = self.resident.iter().find(|(id, _)| *id == used) {
            credential.response.user_handle = Some(user.clone().into());
        }
        Ok(credential)
    }
}
