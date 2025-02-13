extern crate dirs;

use serde::Deserialize;

/// подписание сформированной транзакции с помощью файла с ключами
#[derive(Debug, Default, Clone, clap::Clap)]
#[clap(
    setting(clap::AppSettings::ColoredHelp),
    setting(clap::AppSettings::DisableHelpSubcommand),
    setting(clap::AppSettings::VersionlessSubcommands)
)]
pub struct CliSignKeychain {
    #[clap(long)]
    nonce: Option<u64>,
    #[clap(long)]
    block_hash: Option<near_primitives::hash::CryptoHash>,
    #[clap(subcommand)]
    submit: Option<super::Submit>,
}

#[derive(Debug, Clone)]
pub struct SignKeychain {
    nonce: Option<u64>,
    block_hash: Option<near_primitives::hash::CryptoHash>,
    pub submit: Option<super::Submit>,
}

impl CliSignKeychain {
    pub fn to_cli_args(&self) -> std::collections::VecDeque<String> {
        let mut args = self
            .submit
            .as_ref()
            .map(|subcommand| subcommand.to_cli_args())
            .unwrap_or_default();
        if let Some(nonce) = &self.nonce {
            args.push_front(nonce.to_string());
            args.push_front("--nonce".to_owned())
        }
        if let Some(block_hash) = &self.block_hash {
            args.push_front(block_hash.to_string());
            args.push_front("--block-hash".to_owned())
        }
        args
    }
}

impl From<SignKeychain> for CliSignKeychain {
    fn from(sign_keychain: SignKeychain) -> Self {
        Self {
            nonce: sign_keychain.nonce,
            block_hash: sign_keychain.block_hash,
            submit: sign_keychain.submit,
        }
    }
}

impl SignKeychain {
    pub fn from(
        item: CliSignKeychain,
        connection_config: Option<crate::common::ConnectionConfig>,
        sender_account_id: near_primitives::types::AccountId,
    ) -> color_eyre::eyre::Result<Self> {
        let submit: Option<super::Submit> = item.submit;
        match connection_config {
            Some(_) => Ok(Self {
                nonce: None,
                block_hash: None,
                submit,
            }),
            None => {
                let home_dir = dirs::home_dir().expect("Impossible to get your home dir!");
                let file_name = format!("{}.json", sender_account_id);
                let mut path = std::path::PathBuf::from(&home_dir);
                let dir_name = crate::consts::DIR_NAME_KEY_CHAIN;
                path.push(dir_name);
                path.push(file_name);
                let data = std::fs::read_to_string(path).map_err(|err| {
                    color_eyre::Report::msg(format!("Access key file not found! Error: {}", err))
                })?;
                let account_json: User = serde_json::from_str(&data).map_err(|err| {
                    color_eyre::Report::msg(format!(
                        "Data for the access key was not found in the file! Error: {}",
                        err
                    ))
                })?;

                let nonce: u64 = match item.nonce {
                    Some(cli_nonce) => cli_nonce,
                    None => super::input_access_key_nonce(&account_json.public_key.to_string()),
                };
                let block_hash = match item.block_hash {
                    Some(cli_block_hash) => cli_block_hash,
                    None => super::input_block_hash(),
                };
                Ok(SignKeychain {
                    nonce: Some(nonce),
                    block_hash: Some(block_hash),
                    submit,
                })
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct User {
    account_id: near_primitives::types::AccountId,
    public_key: near_crypto::PublicKey,
    private_key: near_crypto::SecretKey,
}

impl SignKeychain {
    fn rpc_client(&self, selected_server_url: &str) -> near_jsonrpc_client::JsonRpcClient {
        near_jsonrpc_client::new_client(&selected_server_url)
    }

    pub async fn process(
        self,
        prepopulated_unsigned_transaction: near_primitives::transaction::Transaction,
        connection_config: Option<crate::common::ConnectionConfig>,
    ) -> color_eyre::eyre::Result<Option<near_primitives::views::FinalExecutionOutcomeView>> {
        let home_dir = dirs::home_dir().expect("Impossible to get your home dir!");
        let file_name = format!("{}.json", prepopulated_unsigned_transaction.signer_id);
        let mut path = std::path::PathBuf::from(&home_dir);

        let data_path: std::path::PathBuf = match &connection_config {
            None => {
                let dir_name = crate::consts::DIR_NAME_KEY_CHAIN;
                path.push(dir_name);
                path.push(file_name);
                path
            }
            Some(network_connection_config) => {
                let dir_name = network_connection_config.dir_name();
                path.push(dir_name);
                path.push(file_name);

                if path.exists() {
                    path
                } else {
                    let query_view_method_response = self
                        .rpc_client(network_connection_config.rpc_url().as_str())
                        .query(near_jsonrpc_primitives::types::query::RpcQueryRequest {
                            block_reference: near_primitives::types::Finality::Final.into(),
                            request: near_primitives::views::QueryRequest::ViewAccessKeyList {
                                account_id: prepopulated_unsigned_transaction.signer_id.clone(),
                            },
                        })
                        .await
                        .map_err(|err| {
                            color_eyre::Report::msg(format!(
                                "Failed to fetch query for view key list: {:?}",
                                err
                            ))
                        })?;
                    let access_key_view =
                        if let near_jsonrpc_primitives::types::query::QueryResponseKind::AccessKeyList(
                            result,
                        ) = query_view_method_response.kind
                        {
                            result
                        } else {
                            return Err(color_eyre::Report::msg(format!("Error call result")));
                        };
                    let mut path = std::path::PathBuf::from(&home_dir);
                    path.push(dir_name);
                    path.push(&prepopulated_unsigned_transaction.signer_id.to_string());
                    let mut data_path = std::path::PathBuf::new();
                    'outer: for access_key in access_key_view.keys {
                        let account_public_key = access_key.public_key.to_string();
                        let is_full_access_key: bool = match &access_key.access_key.permission {
                            near_primitives::views::AccessKeyPermissionView::FullAccess => true,
                            near_primitives::views::AccessKeyPermissionView::FunctionCall {
                                allowance: _,
                                receiver_id: _,
                                method_names: _,
                            } => false,
                        };
                        let dir = path
                            .read_dir()
                            .map_err(|err| {
                                color_eyre::Report::msg(format!("There are no access keys found in the keychain for the signer account. Log in before signing transactions with keychain. {}", err))
                            })?;
                        for entry in dir {
                            if let Ok(entry) = entry {
                                if entry
                                    .path()
                                    .file_stem()
                                    .unwrap()
                                    .to_str()
                                    .unwrap()
                                    .contains(account_public_key.rsplit(':').next().unwrap())
                                    && is_full_access_key
                                {
                                    data_path.push(entry.path());
                                    break 'outer;
                                }
                            } else {
                                return Err(color_eyre::Report::msg(format!(
                                    "There are no access keys found in the keychain for the signer account. Log in before signing transactions with keychain."
                                )));
                            };
                        }
                    }
                    data_path
                }
            }
        };
        let data = std::fs::read_to_string(data_path).map_err(|err| {
            color_eyre::Report::msg(format!("Access key file not found! Error: {}", err))
        })?;
        let account_json: User = serde_json::from_str(&data)
            .map_err(|err| color_eyre::Report::msg(format!("Error reading data: {}", err)))?;
        let sign_with_private_key = super::sign_with_private_key::SignPrivateKey {
            signer_public_key: account_json.public_key,
            signer_private_key: account_json.private_key,
            nonce: self.nonce.clone(),
            block_hash: self.block_hash.clone(),
            submit: self.submit.clone(),
        };
        sign_with_private_key
            .process(prepopulated_unsigned_transaction, connection_config)
            .await
    }
}
