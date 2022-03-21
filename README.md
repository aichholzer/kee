<div align="center">
  <img src="https://raw.githubusercontent.com/aichholzer/kee/main/kee.png" alt="Kee" />
</div>

![OSX](https://img.shields.io/badge/-OSX-black?logo=apple)<br />
`Kee` will help you to securely manage AWS credentials on your local machine.

* Supports an unlimited number of AWS accounts.
* Credentials are stored in MacOS's `Keychain`, no plain-text nasties.
* Environment variables are only bound to the current session.
* Execute commands in sub-processes (to completely avoid exposing variables to the environment).
* Execute commands with temporary credentials.
* SSO support.

> Temporary credentials are requested from AWS STS and set to expire within fifteen minutes.


### Requirements

 * [AWS CLI](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html)
 * [jq](https://github.com/stedolan/jq)


### Install

```
curl -o- https://raw.githubusercontent.com/aichholzer/kee/main/install.sh | bash
```


### Usage

```
kee [options] [flags]
```


##### Options

 * `add account_name`: Add an account.
 * `use account_name`: Switch to -and use a specific account. This will set AWS environment variables for the current session.
 * `show [account_name]`: Show the account's details. If left blank, it will show the current account.
 * `login [account_name]`: Login to SSO accounts. The login will be performed by the `AWS CLI`.
 * `ls`: List all available accounts.
 * `remove account_name`: Remove the specified account.
 * `export [account_name]`: Export the account's details. If left blank, it will export all accounts.
 * `tf`: Terraform-specific functions.

 > `Kee` supports tab-completion.


##### Flags

 * `-r|--run 'command'`: Environment variables will be exposed to the sub-process which will run the command.
 * `-t|--temp`: Used in conjunction with `--run`. Request a set of temporary credentials and expose them to the sub-process.
 * `--sso`: When adding a new account, set it being SSO. When adding SSO accounts, the `AWS CLI` will be invoked to complete the configuration process.


##### Examples

```
kee add example
...

kee use example --run 'aws s3 ls'
kee use example --run 'aws s3 ls' --temp
```


##### Questions?

RTFM, then RTFC... If you are still stuck or just need an additional feature, file an [issue](https://github.com/aichholzer/kee/issues).

<div align="center">
✌🏼
</div>
