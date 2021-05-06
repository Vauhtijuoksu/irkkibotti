# BotanicsBot

## How to use

Copy `botanist` executable somewhere, write config file

```
./botanicsbot
```

## Config file format

```
{
    "twitch_config" : {
        "nickname" : "whatever_your_twitch_bot_account_name_is",
        "server": "irc.twitch.tv",
        "port": 6667,
        "channels" : ["#comma", "#separated", "#list", "#of", "#channels"],
        "use_tls": false,
        "auth_token": "oauth:twitch_auth_token_for_your_bot_account"
    },
    "channel_configs" : {
        "#list" : {
            "bot_admins" : [
                "people",
                "who",
                "can",
                "modify",
                "bot",
                "commands",
            ],
            "command_blacklist" : [
                "do",
                "not",
                "allow",
                "these",
                "commands",
                "to",
                "be",
                "assigned"
            ]
        }
    }
}
```
