use std::str::FromStr;

// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use common_exception::ErrorCode;

const NO_PASSWORD_STR: &str = "no_password";
const PLAINTEXT_PASSWORD_STR: &str = "plaintext_password";
const SHA256_PASSWORD_STR: &str = "sha256_password";
const DOUBLE_SHA1_PASSWORD_STR: &str = "double_sha1_password";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum AuthType {
    NoPassword,
    PlaintextPassword,
    Sha256Password,
    DoubleShaPassword,
}

impl std::str::FromStr for AuthType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            PLAINTEXT_PASSWORD_STR => Ok(AuthType::PlaintextPassword),
            SHA256_PASSWORD_STR => Ok(AuthType::Sha256Password),
            DOUBLE_SHA1_PASSWORD_STR => Ok(AuthType::DoubleShaPassword),
            NO_PASSWORD_STR => Ok(AuthType::NoPassword),
            _ => Err(AuthType::bad_auth_types(s)),
        }
    }
}

impl AuthType {
    pub fn to_str(&self) -> &str {
        match self {
            AuthType::NoPassword => NO_PASSWORD_STR,
            AuthType::PlaintextPassword => PLAINTEXT_PASSWORD_STR,
            AuthType::Sha256Password => SHA256_PASSWORD_STR,
            AuthType::DoubleShaPassword => DOUBLE_SHA1_PASSWORD_STR,
        }
    }

    fn bad_auth_types(s: &str) -> String {
        let all = vec![
            NO_PASSWORD_STR,
            PLAINTEXT_PASSWORD_STR,
            SHA256_PASSWORD_STR,
            DOUBLE_SHA1_PASSWORD_STR,
        ];
        let all = all
            .iter()
            .map(|s| format!("'{}'", s))
            .collect::<Vec<_>>()
            .join("|");
        format!("Expected auth type {}, found: {}", all, s)
    }

    fn get_password_type(self) -> Option<PasswordType> {
        match self {
            AuthType::PlaintextPassword => Some(PasswordType::PlainText),
            AuthType::Sha256Password => Some(PasswordType::Sha256),
            AuthType::DoubleShaPassword => Some(PasswordType::DoubleSha1),
            _ => None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AuthInfo {
    None,
    Password {
        password: Vec<u8>,
        password_type: PasswordType,
    },
}

fn calc_sha1(v: &[u8]) -> [u8; 20] {
    let mut m = ::sha1::Sha1::new();
    m.update(v);
    m.digest().bytes()
}

fn double_sha1(v: &[u8]) -> [u8; 20] {
    calc_sha1(&calc_sha1(v)[..])
}

impl AuthInfo {
    fn new(auth_type: AuthType, auth_string: &Option<String>) -> Result<AuthInfo, String> {
        match auth_type {
            AuthType::NoPassword => Ok(AuthInfo::None),
            AuthType::PlaintextPassword
            | AuthType::Sha256Password
            | AuthType::DoubleShaPassword => {
                match auth_string {
                    Some(p) => Ok(AuthInfo::Password {
                        password: Vec::from(p.to_string()),
                        // safe unwrap
                        password_type: auth_type.get_password_type().unwrap(),
                    }),
                    None => Err("need password".to_string()),
                }
            }
        }
    }

    pub fn create(
        auth_type: &Option<String>,
        auth_string: &Option<String>,
    ) -> Result<AuthInfo, String> {
        let default = AuthType::Sha256Password;
        let auth_type = auth_type
            .clone()
            .map(|s| AuthType::from_str(&s))
            .transpose()?
            .unwrap_or(default);
        AuthInfo::new(auth_type, auth_string)
    }

    pub fn alter(
        &self,
        auth_type: &Option<String>,
        auth_string: &Option<String>,
    ) -> Result<AuthInfo, String> {
        let old_auth_type = self.get_type();
        let new_auth_type = auth_type
            .clone()
            .map(|s| AuthType::from_str(&s))
            .transpose()?
            .unwrap_or(old_auth_type);
        AuthInfo::new(new_auth_type, auth_string)
    }

    pub fn get_type(&self) -> AuthType {
        match self {
            AuthInfo::None => AuthType::NoPassword,
            AuthInfo::Password {
                password: _,
                password_type: t,
            } => match t {
                PasswordType::PlainText => AuthType::PlaintextPassword,
                PasswordType::Sha256 => AuthType::Sha256Password,
                PasswordType::DoubleSha1 => AuthType::DoubleShaPassword,
            },
        }
    }

    pub fn get_password(&self) -> Option<Vec<u8>> {
        match self {
            AuthInfo::Password {
                password: p,
                password_type: _,
            } => Some(p.to_vec()),
            _ => None,
        }
    }

    pub fn get_password_type(&self) -> Option<PasswordType> {
        match self {
            AuthInfo::Password {
                password: _,
                password_type: t,
            } => Some(*t),
            _ => None,
        }
    }

    fn restore_sha1_mysql(
        salt: &[u8],
        input: &[u8],
        user_password: &[u8],
    ) -> Result<Vec<u8>, ErrorCode> {
        let double_sha1 = calc_sha1(&calc_sha1(user_password)[..]);
        // SHA1( password ) XOR SHA1( "20-bytes random data from server" <concat> SHA1( SHA1( password ) ) )
        let mut m = sha1::Sha1::new();
        m.update(salt);
        m.update(&double_sha1[..]);

        let result = m.digest().bytes();
        if input.len() != result.len() {
            return Err(ErrorCode::SHA1CheckFailed("SHA1 check failed"));
        }
        let mut s = Vec::with_capacity(result.len());
        for i in 0..result.len() {
            s.push(input[i] ^ result[i]);
        }
        Ok(s)
    }

    pub fn auth_mysql(&self, password_input: &[u8], salt: &[u8]) -> Result<bool, ErrorCode> {
        match self {
            AuthInfo::None => Ok(true),
            AuthInfo::Password {
                password: p,
                password_type: t,
            } => {
                match t {
                    PasswordType::PlainText => Ok(p == password_input),
                    PasswordType::DoubleSha1 => {
                        let password_sha1 = AuthInfo::restore_sha1_mysql(salt, password_input, p)?;
                        // TODO(youngsofun): we should store double_sha1(password) later
                        Ok(double_sha1(p) == calc_sha1(&password_sha1))
                    }
                    PasswordType::Sha256 => Err(ErrorCode::AuthenticateFailure(
                        "login with sha256_password user for mysql protocol not supported yet.",
                    )),
                }
            }
        }
    }

    // for clickhouse and http basic
    pub fn auth_password_plaintext(
        &self,
        password_input_plaintext: &[u8],
    ) -> Result<bool, ErrorCode> {
        match self {
            AuthInfo::None => Ok(true),
            AuthInfo::Password {
                password: p,
                password_type: _t,
                // TODO(youngsofun): we should store password hash later
            } => Ok(p == password_input_plaintext),
        }
    }
}

impl Default for AuthInfo {
    fn default() -> Self {
        AuthInfo::None
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd,
)]
pub enum PasswordType {
    PlainText = 0,
    DoubleSha1 = 1,
    Sha256 = 2,
}

impl Default for PasswordType {
    fn default() -> Self {
        PasswordType::Sha256
    }
}
