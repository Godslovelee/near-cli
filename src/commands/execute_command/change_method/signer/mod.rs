use dialoguer::Input;

#[derive(Debug, Clone, clap::Clap)]
pub enum CliSendFrom {
    /// Specify a signer
    Signer(CliSender),
}

#[derive(Debug, Clone)]
pub enum SendFrom {
    Signer(Sender),
}

impl CliSendFrom {
    pub fn to_cli_args(&self) -> std::collections::VecDeque<String> {
        match self {
            Self::Signer(subcommand) => {
                let mut args = subcommand.to_cli_args();
                args.push_front("signer".to_owned());
                args
            }
        }
    }
}

impl From<SendFrom> for CliSendFrom {
    fn from(send_from: SendFrom) -> Self {
        match send_from {
            SendFrom::Signer(sender) => Self::Signer(sender.into()),
        }
    }
}
impl SendFrom {
    pub fn from(
        item: CliSendFrom,
        connection_config: Option<crate::common::ConnectionConfig>,
    ) -> color_eyre::eyre::Result<Self> {
        match item {
            CliSendFrom::Signer(cli_sender) => {
                Ok(Self::Signer(Sender::from(cli_sender, connection_config)?))
            }
        }
    }
}

impl SendFrom {
    pub fn choose_send_from(
        connection_config: Option<crate::common::ConnectionConfig>,
    ) -> color_eyre::eyre::Result<Self> {
        Self::from(CliSendFrom::Signer(Default::default()), connection_config)
    }

    pub async fn process(
        self,
        prepopulated_unsigned_transaction: near_primitives::transaction::Transaction,
        network_connection_config: Option<crate::common::ConnectionConfig>,
    ) -> crate::CliResult {
        match self {
            SendFrom::Signer(sender) => {
                sender
                    .process(prepopulated_unsigned_transaction, network_connection_config)
                    .await
            }
        }
    }
}

/// Specify a signer
#[derive(Debug, Default, Clone, clap::Clap)]
#[clap(
    setting(clap::AppSettings::ColoredHelp),
    setting(clap::AppSettings::DisableHelpSubcommand),
    setting(clap::AppSettings::VersionlessSubcommands)
)]
pub struct CliSender {
    pub sender_account_id: Option<near_primitives::types::AccountId>,
    #[clap(subcommand)]
    pub sign_option: Option<
        crate::commands::construct_transaction_command::sign_transaction::CliSignTransaction,
    >,
}

#[derive(Debug, Clone)]
pub struct Sender {
    pub sender_account_id: near_primitives::types::AccountId,
    pub sign_option:
        crate::commands::construct_transaction_command::sign_transaction::SignTransaction,
}

impl CliSender {
    pub fn to_cli_args(&self) -> std::collections::VecDeque<String> {
        let mut args = self
            .sign_option
            .as_ref()
            .map(|subcommand| subcommand.to_cli_args())
            .unwrap_or_default();
        if let Some(sender_account_id) = &self.sender_account_id {
            args.push_front(sender_account_id.to_string());
        }
        args
    }
}

impl From<Sender> for CliSender {
    fn from(sender: Sender) -> Self {
        Self {
            sender_account_id: Some(sender.sender_account_id),
            sign_option: Some(sender.sign_option.into()),
        }
    }
}

impl Sender {
    fn from(
        item: CliSender,
        connection_config: Option<crate::common::ConnectionConfig>,
    ) -> color_eyre::eyre::Result<Self> {
        let sender_account_id: near_primitives::types::AccountId = match item.sender_account_id {
            Some(cli_sender_account_id) => match &connection_config {
                Some(network_connection_config) => match crate::common::check_account_id(
                    network_connection_config.clone(),
                    cli_sender_account_id.clone(),
                )? {
                    Some(_) => cli_sender_account_id,
                    None => {
                        println!("Account <{}> doesn't exist", cli_sender_account_id);
                        Sender::input_sender_account_id(connection_config.clone())?
                    }
                },
                None => cli_sender_account_id,
            },
            None => Sender::input_sender_account_id(connection_config.clone())?,
        };
        let sign_option = match item.sign_option {
            Some(cli_sign_transaction) => crate::commands::construct_transaction_command::sign_transaction::SignTransaction::from(cli_sign_transaction, connection_config, sender_account_id.clone())?,
            None => crate::commands::construct_transaction_command::sign_transaction::SignTransaction::choose_sign_option(connection_config, sender_account_id.clone())?,
        };
        Ok(Self {
            sender_account_id,
            sign_option,
        })
    }
}

impl Sender {
    fn input_sender_account_id(
        connection_config: Option<crate::common::ConnectionConfig>,
    ) -> color_eyre::eyre::Result<near_primitives::types::AccountId> {
        loop {
            let account_id: near_primitives::types::AccountId = Input::new()
                .with_prompt("What is the account ID of the signer?")
                .interact_text()
                .unwrap();
            if let Some(connection_config) = &connection_config {
                if let Some(_) =
                    crate::common::check_account_id(connection_config.clone(), account_id.clone())?
                {
                    break Ok(account_id);
                } else {
                    println!("Account <{}> doesn't exist", account_id.to_string());
                }
            } else {
                break Ok(account_id);
            }
        }
    }

    pub async fn process(
        self,
        prepopulated_unsigned_transaction: near_primitives::transaction::Transaction,
        network_connection_config: Option<crate::common::ConnectionConfig>,
    ) -> crate::CliResult {
        let unsigned_transaction = near_primitives::transaction::Transaction {
            signer_id: self.sender_account_id.clone(),
            ..prepopulated_unsigned_transaction
        };
        match self
            .sign_option
            .process(unsigned_transaction, network_connection_config.clone())
            .await?
        {
            Some(transaction_info) => {
                crate::common::print_transaction_status(
                    transaction_info,
                    network_connection_config,
                )
                .await;
            }
            None => {}
        };
        Ok(())
    }
}
