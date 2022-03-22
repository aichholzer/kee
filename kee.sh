function __mask () {
  local n=5                        # Take this many characters
  local a="${1:0:${#1}-n}"         # Take all characters, except the last "n"
  local b="${1:${#1}-n}"           # Take the the last "n" characters

  local VALUE=$(printf "%s%s\n" "${a//?/*}" "${b}") # Replace "char" with *
  ! [[ ${2} =~ '^[0-9]+$' ]] && echo ${VALUE} || echo ${VALUE} | tail -c ${2}
}

function __storeAccounts () {
  security add-generic-password -C note -a kee.sh -s kee.sh -U -w ${1}
}

function __loadAccounts () {
  local DEFAULT_RESPONSE=${1}
  local ACCOUNTS=$(security find-generic-password -a kee.sh -s kee.sh -w 2>/dev/null)
  [ ! "${ACCOUNTS}" ] && [ "${1}" ] && echo ${1} || echo ${ACCOUNTS}
}

function __loadAccount () {
  local PROFILE=${1}
  ACCOUNTS=$(__loadAccounts)
  [ ! "${ACCOUNTS}" ] && echo "" && return

  local ACCOUNT=$(echo ${ACCOUNTS} | jq -r '.[] | select(.profile=="'${PROFILE}'")')
  echo ${ACCOUNT}
}

function __getProp () {
  local VALUE=$(echo ${1} | jq -r '.properties.'${2}' | select (. != null)')
  [ "${3}" = "mask" ] && echo $(__mask ${VALUE} ${4}) || echo ${VALUE}
}

function __bold () {
  echo -e "\033[1m${1}\033[0m"
}

function kee () {
  local COMMAND=${1}
  local PROFILE=${2}
  local ACCOUNT
  local ACCOUNTS
  local RUN
  local TEMP=false
  local TF_AUTO_APPROVE=""

  ## Parse special CLI arguments.
  while [ $# -gt 0 ]; do
    case "${1}" in
      -r|--run)
        RUN="${2}"
        ;;

      -t|--temp)
        TEMP=true
        ;;

      --approve)
        TF_AUTO_APPROVE="-auto-approve"
        ;;

      --sso)
        SSO=true
        ;;

      *)
    esac
    shift
  done

  if [ "${COMMAND}" = "ls" ]; then
    ## TODO: Add `[sso]` to listed SSO accounts.
    echo ""
    ACCOUNTS=$(echo $(__loadAccounts) | jq -r '.[] | .profile' | sed "s/\w*/ • /")
    [ ! "${ACCOUNTS}" ] && echo " 💥 No accounts have been found.\n    Get started with: kee add ..." || echo ${ACCOUNTS}
  elif [ "${COMMAND}" = "show" ]; then
    if [ ! "${PROFILE}" ] && [ ! "${AWS_PROFILE}" ]; then
      echo "\n 💥 No profile is currently selected."
    else
      [ ! "${PROFILE}" ] && PROFILE=${AWS_PROFILE}
      ACCOUNT=$(__loadAccount ${PROFILE})
      [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist." && return

      local ACCOUNT_TYPE=$(__getProp ${ACCOUNT} type)

      echo
      echo " • $(__bold Profile:) \t${PROFILE}" $([ "${ACCOUNT_TYPE}" = "sso" ] && echo "[${ACCOUNT_TYPE}]")
      echo " • $(__bold Account ID:) \t$(__getProp ${ACCOUNT} account)"
      echo " • $(__bold Region:) \t$(__getProp ${ACCOUNT} region)"
      echo " • $(__bold Environment:) $(__getProp ${ACCOUNT} environment)"

      local DOMAIN=$(__getProp ${ACCOUNT} domain)
      [ "${DOMAIN}" ] && echo " • $(__bold Domain:) \t\t${DOMAIN}"

      if [ ! "${ACCOUNT_TYPE}" = "sso" ]; then
        echo " • $(__bold Access key:) \t$(__getProp ${ACCOUNT} access_key mask 15)"
        echo " • $(__bold Secret access key:) \t$(__getProp ${ACCOUNT} secret_access_key mask 15)"
      else
        echo " • $(__bold Role name:) \t$(__getProp ${ACCOUNT} role_name)"
        echo " • $(__bold Start url:) \t$(__getProp ${ACCOUNT} start_url)"
      fi
    fi

    return
  elif [ "${COMMAND}" = "add" ]; then
    [ ! "${PROFILE}" ] && echo "\n 💥 You need to specify an account name." && return

    echo "\n Profile name: ${PROFILE}"
    read "REGION? Region (Default: "ap-southeast-2"): "
    read "OUTPUT? Output (Default: "json"): "
    read "DOMAIN? Domain: "
    read "ENVIRONMENT? Environment (Default: "dev"): "

    [ ! "${REGION}" ] && REGION=ap-southeast-2
    [ ! "${OUTPUT}" ] && OUTPUT=json
    [ ! "${ENVIRONMENT}" ] && ENVIRONMENT=dev

    if [ "${SSO}" ]; then
      echo "\n Configure your SSO account: ${PROFILE}"

      aws configure sso --profile ${PROFILE}

      ACCOUNT_ID=`aws configure get sso_account_id --profile ${PROFILE}`
      START_URL=`aws configure get sso_start_url --profile ${PROFILE}`
      ROLE_NAME=`aws configure get sso_role_name --profile ${PROFILE}`

      ACCOUNTS=$(echo $(__loadAccounts '[]') | jq -c '. + [{
        "profile": "'${PROFILE}'",
        "properties": {
          "type": "sso",
          "account": "'${ACCOUNT_ID}'",
          "start_url": "'${START_URL}'",
          "region": "'${REGION}'",
          "output": "'${OUTPUT}'",
          "domain": "'${DOMAIN}'",
          "environment": "'${ENVIRONMENT}'",
          "role_name": "'${ROLE_NAME}'"
        }
      }]')

      __storeAccounts ${ACCOUNTS}

    else
      read "ACCOUNT_ID? Account ID: "
      read "ACCESS_KEY? Access key: "
      read "SECRET_ACCESS_KEY? Secret access key: "

      ACCOUNTS=$(echo $(__loadAccounts '[]') | jq -c '. + [{
        "profile": "'${PROFILE}'",
        "properties": {
          "account": "'${ACCOUNT_ID}'",
          "access_key": "'${ACCESS_KEY}'",
          "secret_access_key": "'${SECRET_ACCESS_KEY}'",
          "region": "'${REGION}'",
          "output": "'${OUTPUT}'",
          "domain": "'${DOMAIN}'",
          "environment": "'${ENVIRONMENT}'"
        }
      }]')

      __storeAccounts ${ACCOUNTS}

      ## Write the account to the AWS CLI config files.
      echo "\n[profile ${PROFILE}]\nregion = ${REGION}\noutput = ${OUTPUT}" >> ~/.aws/config
      echo "\n[${PROFILE}]\naws_access_key_id = ${ACCESS_KEY}\naws_secret_access_key = ${SECRET_ACCESS_KEY}" >> ~/.aws/credentials
    fi

  elif [ "${COMMAND}" = "remove" ]; then
    ACCOUNT=$(__loadAccount ${PROFILE})
    [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist, nothing to remove..." && return

    echo "\n ⚠️  This will permanently remove \"${PROFILE}\""
    read "CONTINUE? Type \"yes\" to continue: "

    if [ "${CONTINUE}" = "yes" ]; then
      ACCOUNTS=$(echo $(__loadAccounts) | jq -c 'del(.[] | select(.profile == "'${PROFILE}'"))')
      __storeAccounts ${ACCOUNTS}
      echo "\n \"${PROFILE}\" has been removed."

      # If the profile being removed is the currently selected profile, then clear it.
      if [ "${PROFILE}" = "${AWS_PROFILE}" ]; then
        unset AWS_PROFILE AWS_DEFAULT_PROFILE
        unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
      fi

      ## Remove the account from the AWS CLI config files.
      sed -i '' '/\[profile '${PROFILE}'\]/,/^$/d' ~/.aws/*
      sed -i '' '/\['${PROFILE}'\]/,/^$/d' ~/.aws/*
      sed -i '' 'N;/^\n$/d;P;D' ~/.aws/*

      kee ls
    fi
  elif [ "${COMMAND}" = "use" ]; then
    ACCOUNT=$(__loadAccount ${PROFILE})
    [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist." && return

    export AWS_PROFILE=${PROFILE}
    export AWS_DEFAULT_PROFILE=${PROFILE}
    export AWS_ACCOUNT_ID=$(__getProp ${ACCOUNT} account)
    export AWS_REGION=$(__getProp ${ACCOUNT} region)
    export AWS_ACCESS_KEY_ID=$(__getProp ${ACCOUNT} access_key)
    export AWS_SECRET_ACCESS_KEY=$(__getProp ${ACCOUNT} secret_access_key)
    export DOMAIN=$(__getProp ${ACCOUNT} domain)
    export ENVIRONMENT=$(__getProp ${ACCOUNT} environment)
    export TERRAFORM_BUCKET=$(__getProp ${ACCOUNT} terraform_bucket)

    echo "\n ✔ Now using profile \"${PROFILE}\""

    ## If we are in a Terraform directory, automatically initialize with the current environment.
    if [ -f "./main.tf" ]; then
        echo "   Initializing the current Terraform environment: \"${ENVIRONMENT}\""
        kee tf
    fi
  elif [ "${COMMAND}" = "login" ]; then
    ACCOUNT=$(__loadAccount ${PROFILE})
    [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist." && return
    local ACCOUNT_TYPE=$(__getProp ${ACCOUNT} type)
    [ ! "${ACCOUNT_TYPE}" = "sso" ] && echo "\n 💥 This is not an SSO account, can't login." && return

    aws sso login --profile ${PROFILE} > /dev/null &&
    aws sts get-caller-identity > /dev/null &&

    export AWS_PROFILE=${PROFILE}
    export AWS_DEFAULT_PROFILE=${PROFILE}
    export AWS_ACCOUNT_ID=$(__getProp ${ACCOUNT} account)
    export AWS_REGION=$(__getProp ${ACCOUNT} region)
    export DOMAIN=$(__getProp ${ACCOUNT} domain)
    export ENVIRONMENT=$(__getProp ${ACCOUNT} environment)
    export TERRAFORM_BUCKET=$(__getProp ${ACCOUNT} terraform_bucket)

    echo "\n ✔ Now logged in and using SSO profile \"${PROFILE}\""
  elif [ "${COMMAND}" = "export" ]; then
    local SAFETY_NOTICE="\n ⚠️  You are about to export PLACEHOLDER. Make sure you keep this output safe & secure."
    if [ ! "${PROFILE}" ]; then
      SAFETY_NOTICE=$(echo ${SAFETY_NOTICE} | sed -r 's/[PLACEHOLDER]+/all accounts/g')
    else
      SAFETY_NOTICE=$(echo ${SAFETY_NOTICE} | sed -r 's/[PLACEHOLDER]+/the '\""${PROFILE}"\"' account/g')
    fi

    echo ${SAFETY_NOTICE}
    read "EXPORT_FILE?    Enter a file name (Default: "kee.json"): "
    [ ! "${EXPORT_FILE}" ] && EXPORT_FILE="kee.json"
    [ ! "${PROFILE}" ] && __loadAccounts '[]' | jq -r '.' > ${EXPORT_FILE} || __loadAccount ${PROFILE} > ${EXPORT_FILE}
  elif [ "${COMMAND}" = "tf" ]; then
    [ ! "${ENVIRONMENT}" ] && echo "\n 💥 The ENVIRONMENT must be set before running this action." && return

    ACTION=${PROFILE}
    [ ! "${ACTION}" ] && ACTION=init

    ACTIONS=("validate plan apply destroy refresh console")
    if [[ " ${ACTIONS[*]} " =~ " ${ACTION} " ]]; then
      terraform ${ACTION} ${TF_AUTO_APPROVE} -var-file=${ENVIRONMENT}.tfvars
    elif [ "${ACTION}" = "init" ]; then
      echo
      READ_BUCKET_NAME=false
      if [ -z "${TERRAFORM_BUCKET}" ]; then
        read "TERRAFORM_BUCKET? State bucket (S3): "
        READ_BUCKET_NAME=true
      fi

      if [ "${TERRAFORM_BUCKET}" ]; then
        terraform init -reconfigure -backend-config="bucket=${TERRAFORM_BUCKET}" | grep -E 'successfully' | sed "s/\w*/ ✔ /"
        if [ "${READ_BUCKET_NAME}" = true ]; then
          echo
          read "SAVE_BUCKET_NAME? Did you want to save this bucket to the current profile? "

          if [ "${SAVE_BUCKET_NAME}" = "yes" ]; then
            ACCOUNTS=$(echo $(__loadAccounts))
            ACCOUNTS=$(echo ${ACCOUNTS} | jq -c 'map((select(.profile=="'${AWS_PROFILE}'") | .properties.terraform_bucket) |= "'${TERRAFORM_BUCKET}'")')
            __storeAccounts ${ACCOUNTS}
          fi
        fi
      else
        terraform init
      fi
    fi
  elif [ "${RUN}" ]; then
    PROFILE=${AWS_PROFILE}
    ACCOUNT=$(__loadAccount ${PROFILE})
    [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist." && return

    RUN=($(echo ${RUN} | tr " " "\n"))

    local TC=""
    local ACCOUNT_TYPE=$(__getProp ${ACCOUNT} type)
    if [ "${ACCOUNT_TYPE}" = "sso" ]; then
      local START_URL=$(__getProp ${ACCOUNT} start_url)
      local ACCOUNT_ID=$(__getProp ${ACCOUNT} account)
      local ROLE_NAME=$(__getProp ${ACCOUNT} role_name)
      local REGION=$(__getProp ${ACCOUNT} region)

      ## Find the session file for the current account,
      ## and extract the access token from it.
      local ACCESS_TOKEN=$(cat $(grep -Rl "${START_URL}" "${HOME}"/.aws/sso/cache/*) | jq -r '.accessToken')

      ## Get the credentials for the SSL role.
      TC=$(echo $(aws sso get-role-credentials --account-id "${ACCOUNT_ID}" --role-name "${ROLE_NAME}" --access-token "${ACCESS_TOKEN}" --region "${REGION}") | jq -r '.roleCredentials')
      TC=(${$(echo ${TC} | jq -r '.accessKeyId, .secretAccessKey, .sessionToken')})

      ## Temporarily write the credentials to the AWS credentials file
      ## Some tools, like Serverless still rely on the information from `.aws/credentials`.
      ## https://github.com/serverless/serverless/issues/7567
      ## https://github.com/aws/aws-sdk-js/issues/2772
      ## https://github.com/serverless-stack/serverless-stack/issues/313
      echo "\n[${PROFILE}]\naws_access_key_id = ${TC[1]}\naws_secret_access_key = ${TC[2]}\naws_session_token = ${TC[3]}" >> ~/.aws/credentials
      (AWS_ACCESS_KEY_ID=${TC[1]} AWS_SECRET_ACCESS_KEY=${TC[2]} AWS_SESSION_TOKEN=${TC[3]} "${RUN[@]}")
    else
      local AK=$(__getProp ${ACCOUNT} access_key)
      local SK=$(__getProp ${ACCOUNT} secret_access_key)

      ## - Obtain a short-lived set of credentials through AWS STS,
      ## - expose the credentials to the sub-process only,
      ## - run the command
      if [ "${TEMP}" = true ]; then
        TC=$(AWS_ACCESS_KEY_ID=${AK} AWS_SECRET_ACCESS_KEY=${SK} aws sts get-session-token --duration-seconds 900 --output json | jq -r '.Credentials')
        TC=(${$(echo ${TC} | jq -r '.AccessKeyId, .SecretAccessKey, .SessionToken')})
        TC=($(echo ${TC} | tr " " "\n"))

        ## See comment above.
        echo "\n[${PROFILE}]\naws_access_key_id = ${TC[1]}\naws_secret_access_key = ${TC[2]}\naws_session_token = ${TC[3]}" >> ~/.aws/credentials
        (AWS_ACCESS_KEY_ID=${TC[1]} AWS_SECRET_ACCESS_KEY=${TC[2]} AWS_SESSION_TOKEN=${TC[3]} "${RUN[@]}")
      else
        ## See comment above.
        echo "\n[${PROFILE}]\naws_access_key_id = ${TC[1]}\naws_secret_access_key = ${TC[2]}" >> ~/.aws/credentials
        (AWS_ACCESS_KEY_ID=${AK} AWS_SECRET_ACCESS_KEY=${SK} "${RUN[@]}")
      fi
    fi

    ## Remove the credentials from the AWS CLI config files.
    sed -i '' '/\[profile '${PROFILE}'\]/,/^$/d' ~/.aws/credentials
    sed -i '' '/\['${PROFILE}'\]/,/^$/d' ~/.aws/credentials
    sed -i '' 'N;/^\n$/d;P;D' ~/.aws/credentials
  else
    echo "\n 💥 You need to give me a command."

    echo "\n Currently active profile:"
    kee show

    echo "\n Available profiles:"
    kee ls

    return
  fi
}
