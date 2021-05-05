pub struct Client {
    irc_client: irc::client::Client,
}

impl Client {
    pub async fn connect() -> Result<Client, anyhow::Error> {
        let adhoc_conf = irc::client::prelude::Config {
            nickname: Some("botanicsbot".to_owned()),
            server: Some("irc.twitch.tv".to_owned()),
            port: Some(6667),
            channels: vec!["#olenananas".to_owned()],
            ..irc::client::prelude::Config::default()
        };

        let client = irc::client::Client::from_config(adhoc_conf).await?;
        client.identify()?;

        Ok(Self {
            irc_client: client
        })
    }
}

/*
    async fn spawn(&self) {
        tokio::spawn(async {
        }).await.unwrap();
    }
*/
